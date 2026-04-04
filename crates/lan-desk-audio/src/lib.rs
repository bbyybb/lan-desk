pub mod opus_codec;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use tracing::{debug, info, warn};

/// 音频捕获配置
pub struct AudioConfig {
    pub sample_rate: u32,
    pub channels: u16,
    pub bits_per_sample: u16,
}

/// 音频捕获句柄，用于控制音频线程的生命周期
pub struct AudioHandle {
    stop_flag: Arc<AtomicBool>,
}

impl AudioHandle {
    /// 停止音频捕获线程
    pub fn stop(&self) {
        self.stop_flag.store(true, Ordering::Relaxed);
    }
}

impl Drop for AudioHandle {
    fn drop(&mut self) {
        self.stop();
    }
}

/// 启动系统音频捕获（loopback），返回 (配置, PCM 数据接收端, 控制句柄)
///
/// 设备选择策略：
///   1. 优先尝试 `default_output_device` 的输入配置（loopback 捕获系统音频）
///   2. 如果 loopback 不可用，回退到 `default_input_device`（普通麦克风）
///
/// 当前传输原始 PCM 16-bit 数据，带宽消耗较高（约 1.5 Mbps stereo 48kHz）。
/// Opus 编码已实现（见 opus_codec.rs，通过 feature gate 启用），可将带宽降至 ~64-128 kbps。
///
/// 注意：macOS 不直接支持 loopback 捕获，需安装虚拟音频设备（如 BlackHole）。
pub fn start_audio_capture() -> anyhow::Result<(AudioConfig, mpsc::Receiver<Vec<u8>>, AudioHandle)>
{
    let host = cpal::default_host();

    // 尝试获取音频设备：优先 loopback（输出设备的输入配置），回退到麦克风
    let (device, config) = select_audio_device(&host)?;

    let sample_rate = config.sample_rate().0;
    let channels = config.channels();
    let sample_format = config.sample_format();

    info!(
        "音频捕获设备: {}, 格式: {:?}, 采样率: {}, 通道: {}",
        device.name().unwrap_or_default(),
        sample_format,
        sample_rate,
        channels,
    );

    let (tx, rx) = mpsc::channel::<Vec<u8>>();
    let stream_config: cpal::StreamConfig = config.into();

    let err_fn = |err| warn!("音频流错误: {}", err);

    let audio_config = AudioConfig {
        sample_rate,
        channels,
        bits_per_sample: 16,
    };

    let stop_flag = Arc::new(AtomicBool::new(false));
    let stop_flag_clone = stop_flag.clone();

    std::thread::Builder::new()
        .name("audio-capture".to_string())
        .spawn(move || {
            let stream = match sample_format {
                cpal::SampleFormat::F32 => {
                    let tx = tx.clone();
                    device.build_input_stream(
                        &stream_config,
                        move |data: &[f32], _: &cpal::InputCallbackInfo| {
                            let mut pcm = Vec::with_capacity(data.len() * 2);
                            for &sample in data {
                                let s = (sample * 32767.0).clamp(-32768.0, 32767.0) as i16;
                                pcm.extend_from_slice(&s.to_le_bytes());
                            }
                            let _ = tx.send(pcm);
                        },
                        err_fn,
                        None,
                    )
                }
                cpal::SampleFormat::I16 => {
                    let tx = tx.clone();
                    device.build_input_stream(
                        &stream_config,
                        move |data: &[i16], _: &cpal::InputCallbackInfo| {
                            let mut pcm = Vec::with_capacity(data.len() * 2);
                            for &sample in data {
                                pcm.extend_from_slice(&sample.to_le_bytes());
                            }
                            let _ = tx.send(pcm);
                        },
                        err_fn,
                        None,
                    )
                }
                cpal::SampleFormat::U16 => {
                    let tx = tx.clone();
                    device.build_input_stream(
                        &stream_config,
                        move |data: &[u16], _: &cpal::InputCallbackInfo| {
                            let mut pcm = Vec::with_capacity(data.len() * 2);
                            for &sample in data {
                                // U16 [0, 65535] -> I16 [-32768, 32767]
                                let s = (sample as i32 - 32768) as i16;
                                pcm.extend_from_slice(&s.to_le_bytes());
                            }
                            let _ = tx.send(pcm);
                        },
                        err_fn,
                        None,
                    )
                }
                cpal::SampleFormat::I32 => {
                    let tx = tx.clone();
                    device.build_input_stream(
                        &stream_config,
                        move |data: &[i32], _: &cpal::InputCallbackInfo| {
                            let mut pcm = Vec::with_capacity(data.len() * 2);
                            for &sample in data {
                                // I32 -> I16: 右移 16 位截断
                                let s = (sample >> 16) as i16;
                                pcm.extend_from_slice(&s.to_le_bytes());
                            }
                            let _ = tx.send(pcm);
                        },
                        err_fn,
                        None,
                    )
                }
                other => {
                    warn!("不支持的音频格式: {:?}", other);
                    return;
                }
            };

            match stream {
                Ok(s) => {
                    if let Err(e) = s.play() {
                        warn!("播放音频流失败: {}", e);
                        return;
                    }
                    debug!("音频捕获已启动");
                    // 定期检查停止信号，而非无限睡眠
                    while !stop_flag_clone.load(Ordering::Relaxed) {
                        std::thread::sleep(std::time::Duration::from_millis(500));
                    }
                    debug!("音频捕获线程收到停止信号，退出");
                    // stream 在此 drop，自动停止音频捕获
                }
                Err(e) => {
                    warn!("创建音频流失败: {}", e);
                }
            }
        })?;

    let handle = AudioHandle { stop_flag };
    Ok((audio_config, rx, handle))
}

/// 选择音频捕获设备和配置
///
/// 策略：
///   1. 尝试输出设备的默认输入配置（loopback）
///   2. 回退到默认输入设备（麦克风）
fn select_audio_device(
    host: &cpal::Host,
) -> anyhow::Result<(cpal::Device, cpal::SupportedStreamConfig)> {
    // 第一优先级：输出设备 + 输入配置（loopback 捕获系统音频）
    if let Some(output_device) = host.default_output_device() {
        let dev_name = output_device.name().unwrap_or_default();
        match output_device.default_input_config() {
            Ok(config) => {
                info!(
                    "使用输出设备 loopback 捕获: {} ({}Hz, {}ch, {:?})",
                    dev_name,
                    config.sample_rate().0,
                    config.channels(),
                    config.sample_format(),
                );
                return Ok((output_device, config));
            }
            Err(e) => {
                warn!(
                    "输出设备 '{}' 不支持 loopback 输入配置: {}，尝试回退到麦克风",
                    dev_name, e,
                );
            }
        }
    } else {
        warn!("没有找到音频输出设备，尝试回退到麦克风输入设备");
    }

    // 第二优先级：默认输入设备（麦克风）
    if let Some(input_device) = host.default_input_device() {
        let dev_name = input_device.name().unwrap_or_default();
        match input_device.default_input_config() {
            Ok(config) => {
                info!(
                    "回退使用麦克风输入设备: {} ({}Hz, {}ch, {:?})",
                    dev_name,
                    config.sample_rate().0,
                    config.channels(),
                    config.sample_format(),
                );
                return Ok((input_device, config));
            }
            Err(e) => {
                warn!("麦克风设备 '{}' 获取配置失败: {}", dev_name, e);
            }
        }
    } else {
        warn!("没有找到默认输入设备（麦克风）");
    }

    anyhow::bail!("没有可用的音频捕获设备（loopback 和麦克风均不可用）")
}
