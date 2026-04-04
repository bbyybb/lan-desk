import { type Ref } from "vue";

export interface FrameRegion {
  x: number;
  y: number;
  width: number;
  height: number;
  encoding: string;
  data_url: string;
}

/**
 * 帧渲染 composable -- 管理 canvas 上下文、H.264 VideoDecoder、JPEG/Raw 绘制
 */
export function useFrameRenderer(canvas: Ref<HTMLCanvasElement | null>) {
  let ctx: CanvasRenderingContext2D | null = null;
  let videoDecoder: VideoDecoder | null = null;
  let hevcDecoder: VideoDecoder | null = null;
  let av1Decoder: VideoDecoder | null = null;
  let h264TimestampCounter = 0;
  let hevcTimestampCounter = 0;
  let av1TimestampCounter = 0;
  const FRAME_DURATION_US = 33333;

  let remoteWidth = 1920;
  let remoteHeight = 1080;
  let canvasInited = false;
  let jpegSeqCounter = 0;
  let wsBinaryJpegSeq = 0;

  function getCtx() {
    return ctx;
  }

  function initCanvas(width: number, height: number) {
    const c = canvas.value;
    if (!c) return;
    if (canvasInited && width === remoteWidth && height === remoteHeight) return;
    c.width = width;
    c.height = height;
    remoteWidth = width;
    remoteHeight = height;
    canvasInited = true;
  }

  function createDecoderOutput() {
    return (frame: VideoFrame) => {
      if (ctx && canvas.value) {
        ctx.drawImage(frame, 0, 0, canvas.value.width, canvas.value.height);
        frame.close();
      }
    };
  }

  function createDecoderError(name: string) {
    return (e: DOMException) => {
      console.error(`${name} VideoDecoder error:`, e);
    };
  }

  function initVideoDecoder() {
    if (typeof VideoDecoder === "undefined") return;

    // H.264 decoder
    videoDecoder = new VideoDecoder({
      output: createDecoderOutput(),
      error: createDecoderError("H.264"),
    });
    videoDecoder.configure({
      codec: "avc1.42001e",
      optimizeForLatency: true,
    });

    // H.265/HEVC decoder
    initHevcDecoder();

    // AV1 decoder
    initAv1Decoder();
  }

  async function initHevcDecoder() {
    if (typeof VideoDecoder === "undefined") return;
    // hev1.1.6.L93.B0 = HEVC Main Profile, Level 3.1
    const hevcCodec = "hev1.1.6.L93.B0";
    try {
      const support = await VideoDecoder.isConfigSupported({
        codec: hevcCodec,
        optimizeForLatency: true,
      });
      if (support.supported) {
        hevcDecoder = new VideoDecoder({
          output: createDecoderOutput(),
          error: createDecoderError("HEVC"),
        });
        hevcDecoder.configure({
          codec: hevcCodec,
          optimizeForLatency: true,
        });
        // HEVC decoder ready
      } else {
        console.warn("HEVC decoder not supported by browser");
      }
    } catch (e) {
      console.warn("HEVC decoder init failed:", e);
    }
  }

  async function initAv1Decoder() {
    if (typeof VideoDecoder === "undefined") return;
    const av1Codec = "av01.0.04M.08";
    try {
      const support = await VideoDecoder.isConfigSupported({
        codec: av1Codec,
        optimizeForLatency: true,
      });
      if (support.supported) {
        av1Decoder = new VideoDecoder({
          output: createDecoderOutput(),
          error: createDecoderError("AV1"),
        });
        av1Decoder.configure({
          codec: av1Codec,
          optimizeForLatency: true,
        });
        // AV1 decoder ready
      } else {
        console.warn("AV1 decoder not supported by browser");
      }
    } catch (e) {
      console.warn("AV1 decoder init failed:", e);
    }
  }

  /** 设置初始 canvas 上下文并绘制等待文本 */
  function setupCanvas(waitingText: string) {
    const c = canvas.value;
    if (!c) return;
    c.width = remoteWidth;
    c.height = remoteHeight;
    ctx = c.getContext("2d");
    if (ctx) {
      ctx.fillStyle = "#1a1a2e";
      ctx.fillRect(0, 0, c.width, c.height);
      ctx.fillStyle = "#666";
      ctx.font = "18px sans-serif";
      ctx.textAlign = "center";
      ctx.fillText(waitingText, c.width / 2, c.height / 2);
    }
  }

  /** Tauri IPC 路径：从 data_url (base64) 绘制区域 */
  function drawRegion(region: FrameRegion, addBytes: (n: number) => void) {
    if (!ctx || !canvas.value) return;

    addBytes(Math.floor(region.data_url.length * 0.75));

    if (region.encoding.startsWith("H264")) {
      if (videoDecoder && videoDecoder.state === "configured") {
        const raw = region.data_url;
        const bytes = Uint8Array.from(atob(raw), (c) => c.charCodeAt(0));
        const isKey = region.encoding.includes("true");
        const chunk = new EncodedVideoChunk({
          type: isKey ? "key" : "delta",
          timestamp: (h264TimestampCounter += FRAME_DURATION_US),
          data: bytes,
        });
        try {
          videoDecoder.decode(chunk);
        } catch (e) {
          console.warn("H.264 decode error (IPC):", e);
        }
      }
    } else if (region.encoding.startsWith("H265")) {
      if (hevcDecoder && hevcDecoder.state === "configured") {
        const raw = region.data_url;
        const bytes = Uint8Array.from(atob(raw), (c) => c.charCodeAt(0));
        const isKey = region.encoding.includes("true");
        const chunk = new EncodedVideoChunk({
          type: isKey ? "key" : "delta",
          timestamp: (hevcTimestampCounter += FRAME_DURATION_US),
          data: bytes,
        });
        try {
          hevcDecoder.decode(chunk);
        } catch (e) {
          console.warn("HEVC decode error (IPC):", e);
        }
      }
    } else if (region.encoding.startsWith("Av1")) {
      if (av1Decoder && av1Decoder.state === "configured") {
        const raw = region.data_url;
        const bytes = Uint8Array.from(atob(raw), (c) => c.charCodeAt(0));
        const isKey = region.encoding.includes("true");
        const chunk = new EncodedVideoChunk({
          type: isKey ? "key" : "delta",
          timestamp: (av1TimestampCounter += FRAME_DURATION_US),
          data: bytes,
        });
        try {
          av1Decoder.decode(chunk);
        } catch (e) {
          console.warn("AV1 decode error (IPC):", e);
        }
      }
    } else if (region.encoding.startsWith("Jpeg")) {
      const seq = ++jpegSeqCounter;
      const img = new Image();
      img.onload = () => {
        if (seq === jpegSeqCounter) {
          ctx?.drawImage(img, region.x, region.y, region.width, region.height);
        }
      };
      img.src = region.data_url;
    } else {
      // Raw RGB
      const binaryStr = atob(region.data_url);
      const data = new Uint8Array(binaryStr.length);
      for (let k = 0; k < binaryStr.length; k++) data[k] = binaryStr.charCodeAt(k);
      const pixelCount = region.width * region.height;
      const imageData = ctx.createImageData(region.width, region.height);
      const dst32 = new Uint32Array(imageData.data.buffer);
      for (let i = 0, s = 0; i < pixelCount; i++, s += 3) {
        dst32[i] = 0xff000000 | (data[s + 2] << 16) | (data[s + 1] << 8) | data[s];
      }
      ctx.putImageData(imageData, region.x, region.y);
    }
  }

  /** WebSocket 二进制路径：从 Uint8Array 直接绘制区域 */
  function drawBinaryRegion(
    x: number,
    y: number,
    w: number,
    h: number,
    encType: number,
    encMeta: number,
    data: Uint8Array,
  ) {
    if (!ctx || !canvas.value) return;

    if (encType === 2) {
      // H.264
      if (videoDecoder && videoDecoder.state === "configured") {
        const isKey = encMeta === 1;
        const chunk = new EncodedVideoChunk({
          type: isKey ? "key" : "delta",
          timestamp: (h264TimestampCounter += FRAME_DURATION_US),
          data: data,
        });
        try {
          videoDecoder.decode(chunk);
        } catch (e) {
          console.warn("H.264 decode error (WS):", e);
        }
      }
    } else if (encType === 3) {
      // H.265/HEVC
      if (hevcDecoder && hevcDecoder.state === "configured") {
        const isKey = encMeta === 1;
        const chunk = new EncodedVideoChunk({
          type: isKey ? "key" : "delta",
          timestamp: (hevcTimestampCounter += FRAME_DURATION_US),
          data: data,
        });
        try {
          hevcDecoder.decode(chunk);
        } catch (e) {
          console.warn("HEVC decode error (WS):", e);
        }
      }
    } else if (encType === 4) {
      // AV1
      if (av1Decoder && av1Decoder.state === "configured") {
        const isKey = encMeta === 1;
        const chunk = new EncodedVideoChunk({
          type: isKey ? "key" : "delta",
          timestamp: (av1TimestampCounter += FRAME_DURATION_US),
          data: data,
        });
        try {
          av1Decoder.decode(chunk);
        } catch (e) {
          console.warn("AV1 decode error (WS):", e);
        }
      }
    } else if (encType === 0) {
      // JPEG
      const mySeq = ++wsBinaryJpegSeq;
      const blob = new Blob([new Uint8Array(data)], { type: "image/jpeg" });
      const url = URL.createObjectURL(blob);
      const img = new Image();
      img.onload = () => {
        if (mySeq === wsBinaryJpegSeq) {
          ctx?.drawImage(img, x, y, w, h);
        }
        URL.revokeObjectURL(url);
      };
      img.onerror = () => URL.revokeObjectURL(url);
      img.src = url;
    } else {
      // Raw RGB
      const pixelCount = w * h;
      const imageData = ctx.createImageData(w, h);
      const dst32 = new Uint32Array(imageData.data.buffer);
      for (let i = 0, s = 0; i < pixelCount; i++, s += 3) {
        dst32[i] = 0xff000000 | (data[s + 2] << 16) | (data[s + 1] << 8) | data[s];
      }
      ctx.putImageData(imageData, x, y);
    }
  }

  /** 自动调整 canvas 尺寸（全屏区域或帧首次到达时初始化） */
  function autoResize(x: number, y: number, w: number, h: number) {
    if (w > 100 && h > 100) {
      if (x === 0 && y === 0) {
        initCanvas(w, h);
      } else if (!canvasInited) {
        initCanvas(x + w, y + h);
      }
    }
  }

  function destroy() {
    if (videoDecoder && videoDecoder.state !== "closed") {
      try { videoDecoder.close(); } catch (_) { /* ignored */ }
    }
    if (hevcDecoder && hevcDecoder.state !== "closed") {
      try { hevcDecoder.close(); } catch (_) { /* ignored */ }
    }
    if (av1Decoder && av1Decoder.state !== "closed") {
      try { av1Decoder.close(); } catch (_) { /* ignored */ }
    }
    videoDecoder = null;
    hevcDecoder = null;
    av1Decoder = null;
    ctx = null;
  }

  return {
    getCtx,
    setupCanvas,
    initCanvas,
    initVideoDecoder,
    drawRegion,
    drawBinaryRegion,
    autoResize,
    destroy,
  };
}
