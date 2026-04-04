use bytes::{Buf, BufMut, BytesMut};
use tokio_util::codec::{Decoder, Encoder};

use crate::message::Message;

/// TCP 帧编解码器：4 字节长度前缀 + MessagePack 序列化的 Message
///
/// 帧格式：
/// ```text
/// +-------------------+------------------------+
/// | 帧长度 (4 bytes)  | 载荷 (MessagePack)     |
/// | u32 big-endian    | 变长                   |
/// +-------------------+------------------------+
/// ```
pub struct LanDeskCodec {
    max_frame_size: usize,
}

impl LanDeskCodec {
    pub fn new() -> Self {
        Self {
            // 默认最大帧 16MB，足以承载一帧 1080p JPEG
            max_frame_size: 16 * 1024 * 1024,
        }
    }

    pub fn with_max_frame_size(max_frame_size: usize) -> Self {
        Self { max_frame_size }
    }
}

impl Default for LanDeskCodec {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CodecError {
    #[error("帧大小超出限制: {size} > {max}")]
    FrameTooLarge { size: usize, max: usize },
    #[error("MessagePack 序列化错误: {0}")]
    MsgPack(String),
    #[error("IO 错误: {0}")]
    Io(#[from] std::io::Error),
}

impl Decoder for LanDeskCodec {
    type Item = Message;
    type Error = CodecError;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        // 至少需要 4 字节的长度前缀
        if src.len() < 4 {
            return Ok(None);
        }

        // 读取帧长度（不消费字节）
        let len = u32::from_be_bytes([src[0], src[1], src[2], src[3]]) as usize;

        // 检查帧大小限制
        if len > self.max_frame_size {
            return Err(CodecError::FrameTooLarge {
                size: len,
                max: self.max_frame_size,
            });
        }

        // 数据还不够完整
        if src.len() < 4 + len {
            // 预留空间，减少重新分配
            src.reserve(4 + len - src.len());
            return Ok(None);
        }

        // 跳过长度前缀
        src.advance(4);
        // 取出载荷
        let payload = src.split_to(len);
        // 反序列化
        let msg: Message =
            rmp_serde::from_slice(&payload).map_err(|e| CodecError::MsgPack(e.to_string()))?;
        Ok(Some(msg))
    }
}

impl Encoder<Message> for LanDeskCodec {
    type Error = CodecError;

    fn encode(&mut self, item: Message, dst: &mut BytesMut) -> Result<(), Self::Error> {
        let payload =
            rmp_serde::to_vec_named(&item).map_err(|e| CodecError::MsgPack(e.to_string()))?;

        if payload.len() > self.max_frame_size {
            return Err(CodecError::FrameTooLarge {
                size: payload.len(),
                max: self.max_frame_size,
            });
        }

        dst.reserve(4 + payload.len());
        dst.put_u32(payload.len() as u32);
        dst.extend_from_slice(&payload);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::*;

    fn roundtrip(msg: Message) -> Message {
        let mut codec = LanDeskCodec::new();
        let mut buf = BytesMut::new();
        codec.encode(msg, &mut buf).unwrap();
        codec.decode(&mut buf).unwrap().unwrap()
    }

    #[test]
    fn test_ping_pong_roundtrip() {
        let decoded = roundtrip(Message::Ping {
            timestamp_ms: 12345,
        });
        assert!(matches!(
            decoded,
            Message::Ping {
                timestamp_ms: 12345
            }
        ));

        let decoded = roundtrip(Message::Pong {
            timestamp_ms: 99999,
        });
        assert!(matches!(
            decoded,
            Message::Pong {
                timestamp_ms: 99999
            }
        ));
    }

    #[test]
    fn test_hello_roundtrip() {
        let msg = Message::Hello {
            version: 1,
            hostname: "test-pc".to_string(),
            screen_width: 1920,
            screen_height: 1080,
            pin: "123456".to_string(),
            pin_salt: "abcdef0123456789".to_string(),
            dpi_scale: 150,
            requested_role: SessionRole::Controller,
        };
        let decoded = roundtrip(msg);
        match decoded {
            Message::Hello {
                version,
                hostname,
                screen_width,
                pin,
                pin_salt,
                dpi_scale,
                ..
            } => {
                assert_eq!(version, 1);
                assert_eq!(hostname, "test-pc");
                assert_eq!(screen_width, 1920);
                assert_eq!(pin, "123456");
                assert_eq!(pin_salt, "abcdef0123456789");
                assert_eq!(dpi_scale, 150);
            }
            _ => panic!("期望 Hello"),
        }
    }

    #[test]
    fn test_hello_ack_roundtrip() {
        let msg = Message::HelloAck {
            version: 1,
            accepted: false,
            reject_reason: "密码错误".to_string(),
            granted_role: SessionRole::Viewer,
        };
        let decoded = roundtrip(msg);
        match decoded {
            Message::HelloAck {
                accepted,
                reject_reason,
                ..
            } => {
                assert!(!accepted);
                assert_eq!(reject_reason, "密码错误");
            }
            _ => panic!("期望 HelloAck"),
        }
    }

    #[test]
    fn test_frame_data_roundtrip() {
        let msg = Message::FrameData {
            seq: 42,
            timestamp_ms: 1000,
            regions: vec![DirtyRegion {
                x: 0,
                y: 0,
                width: 64,
                height: 64,
                encoding: FrameEncoding::Jpeg { quality: 75 },
                data: vec![0xFF; 100],
            }],
            cursor_x: 0.5,
            cursor_y: 0.3,
            cursor_shape: CursorShape::Hand,
        };
        let decoded = roundtrip(msg);
        match decoded {
            Message::FrameData {
                seq,
                regions,
                cursor_shape,
                ..
            } => {
                assert_eq!(seq, 42);
                assert_eq!(regions.len(), 1);
                assert_eq!(regions[0].width, 64);
                assert_eq!(regions[0].data.len(), 100);
                assert_eq!(cursor_shape, CursorShape::Hand);
            }
            _ => panic!("期望 FrameData"),
        }
    }

    #[test]
    fn test_input_events_roundtrip() {
        let msg = roundtrip(Message::MouseMove { x: 0.5, y: 0.75 });
        assert!(
            matches!(msg, Message::MouseMove { x, y } if (x - 0.5).abs() < 0.001 && (y - 0.75).abs() < 0.001)
        );

        let msg = roundtrip(Message::KeyEvent {
            code: "KeyA".to_string(),
            pressed: true,
            modifiers: 0x03,
        });
        match msg {
            Message::KeyEvent {
                code,
                pressed,
                modifiers,
            } => {
                assert_eq!(code, "KeyA");
                assert!(pressed);
                assert_eq!(modifiers, 0x03);
            }
            _ => panic!("期望 KeyEvent"),
        }
    }

    #[test]
    fn test_file_transfer_roundtrip() {
        let msg = roundtrip(Message::FileTransferStart {
            filename: "test.txt".to_string(),
            size: 1024,
            transfer_id: 1,
        });
        match msg {
            Message::FileTransferStart {
                filename,
                size,
                transfer_id,
            } => {
                assert_eq!(filename, "test.txt");
                assert_eq!(size, 1024);
                assert_eq!(transfer_id, 1);
            }
            _ => panic!("期望 FileTransferStart"),
        }
    }

    #[test]
    fn test_partial_frame() {
        let mut codec = LanDeskCodec::new();
        let msg = Message::Pong { timestamp_ms: 0 };

        let mut buf = BytesMut::new();
        codec.encode(msg, &mut buf).unwrap();

        let mut partial = buf.split_to(buf.len() / 2);
        assert!(codec.decode(&mut partial).unwrap().is_none());
    }

    #[test]
    fn test_empty_buffer() {
        let mut codec = LanDeskCodec::new();
        let mut buf = BytesMut::new();
        assert!(codec.decode(&mut buf).unwrap().is_none());
    }

    #[test]
    fn test_frame_too_large() {
        let _codec = LanDeskCodec::with_max_frame_size(10);
        let msg = Message::FrameData {
            seq: 0,
            timestamp_ms: 0,
            regions: vec![DirtyRegion {
                x: 0,
                y: 0,
                width: 1,
                height: 1,
                encoding: FrameEncoding::Raw,
                data: vec![0; 100],
            }],
            cursor_x: 0.0,
            cursor_y: 0.0,
            cursor_shape: CursorShape::Arrow,
        };
        let mut buf = BytesMut::new();
        let mut enc = LanDeskCodec::new();
        enc.encode(msg, &mut buf).unwrap();

        let mut dec = LanDeskCodec::with_max_frame_size(10);
        assert!(dec.decode(&mut buf).is_err());
    }

    #[test]
    fn test_multiple_messages_in_buffer() {
        let mut codec = LanDeskCodec::new();
        let mut buf = BytesMut::new();

        codec
            .encode(Message::Ping { timestamp_ms: 1 }, &mut buf)
            .unwrap();
        codec
            .encode(Message::Pong { timestamp_ms: 2 }, &mut buf)
            .unwrap();
        codec.encode(Message::Disconnect, &mut buf).unwrap();

        let m1 = codec.decode(&mut buf).unwrap().unwrap();
        assert!(matches!(m1, Message::Ping { timestamp_ms: 1 }));

        let m2 = codec.decode(&mut buf).unwrap().unwrap();
        assert!(matches!(m2, Message::Pong { timestamp_ms: 2 }));

        let m3 = codec.decode(&mut buf).unwrap().unwrap();
        assert!(matches!(m3, Message::Disconnect));

        assert!(codec.decode(&mut buf).unwrap().is_none());
    }
}
