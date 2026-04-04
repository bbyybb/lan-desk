/**
 * 屏幕录制 composable
 *
 * 使用 MediaRecorder API 录制 Canvas 内容为 WebM 视频，
 * 优先使用 VP9 编码，支持自动降级。
 * 录制历史保存到 IndexedDB，支持回放和下载。
 */

import { ref } from "vue";

const STORAGE_KEY = "lan-desk-recordings";
const DB_NAME = "lan-desk-recording-db";
const STORE_NAME = "recordings";

export interface RecordingEntry {
  id: string;
  name: string;
  date: number;
  size: number;
}

function openDB(): Promise<IDBDatabase> {
  return new Promise((resolve, reject) => {
    const request = indexedDB.open(DB_NAME, 1);
    request.onupgradeneeded = () => {
      request.result.createObjectStore(STORE_NAME);
    };
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error);
  });
}

async function saveBlob(id: string, blob: Blob): Promise<void> {
  const db = await openDB();
  return new Promise((resolve, reject) => {
    const tx = db.transaction(STORE_NAME, "readwrite");
    tx.objectStore(STORE_NAME).put(blob, id);
    tx.oncomplete = () => resolve();
    tx.onerror = () => reject(tx.error);
  });
}

async function loadBlob(id: string): Promise<Blob | undefined> {
  const db = await openDB();
  return new Promise((resolve, reject) => {
    const tx = db.transaction(STORE_NAME, "readonly");
    const req = tx.objectStore(STORE_NAME).get(id);
    req.onsuccess = () => resolve(req.result as Blob | undefined);
    req.onerror = () => reject(req.error);
  });
}

async function deleteBlob(id: string): Promise<void> {
  const db = await openDB();
  return new Promise((resolve, reject) => {
    const tx = db.transaction(STORE_NAME, "readwrite");
    tx.objectStore(STORE_NAME).delete(id);
    tx.oncomplete = () => resolve();
    tx.onerror = () => reject(tx.error);
  });
}

/**
 * 修复 WebM 文件缺少 Duration 元数据的问题
 * MediaRecorder 生成的 WebM 不包含 Duration，导致播放器无法显示进度条和拖动
 * 通过解析 EBML 结构找到 Segment > Info 部分，注入 Duration 元素
 */
async function fixWebmDuration(blob: Blob, durationMs: number): Promise<Blob> {
  try {
    const buf = await blob.arrayBuffer();
    const data = new Uint8Array(buf);

    // 查找 Segment Info 中的 TimecodeScale (0x2AD7B1) 和注入点
    // WebM EBML ID 标识:
    // 0x1A45DFA3 = EBML header
    // 0x18538067 = Segment
    // 0x1549A966 = Segment Info
    // 0x2AD7B1   = TimecodeScale
    // 0x4489     = Duration (float64, EBML ID)

    // 简化方案：查找 0x44 0x89 (Duration tag) 是否已存在
    for (let i = 0; i < Math.min(data.length, 1024); i++) {
      if (data[i] === 0x44 && i + 1 < data.length && data[i + 1] === 0x89) {
        // Duration 已存在，直接返回原始 blob
        return blob;
      }
    }

    // 查找 Info 段 (0x15 0x49 0xA9 0x66) 并在其内部注入 Duration
    for (let i = 0; i < Math.min(data.length, 512); i++) {
      if (data[i] === 0x15 && data[i + 1] === 0x49 && data[i + 2] === 0xA9 && data[i + 3] === 0x66) {
        // 找到 Info 段，在 Info 段内容开头（跳过 size bytes）注入 Duration
        // Info 段 size 编码：VINT，第一个字节的高位 0 个数 = 额外字节数
        const sizeStart = i + 4;
        const sizeByte = data[sizeStart];
        let sizeLen = 1;
        if ((sizeByte & 0x80) === 0) {
          if ((sizeByte & 0x40) !== 0) sizeLen = 2;
          else if ((sizeByte & 0x20) !== 0) sizeLen = 3;
          else if ((sizeByte & 0x10) !== 0) sizeLen = 4;
          else sizeLen = 5;
        }
        const insertPos = sizeStart + sizeLen;

        // 构建 Duration 元素：ID(0x4489) + Size(0x88=8bytes) + float64(duration in ms * 1000)
        // TimecodeScale 默认 1000000 (1ms)，Duration 单位为 TimecodeScale
        const durationFloat = new Float64Array([durationMs]);
        const durationBytes = new Uint8Array(durationFloat.buffer);
        // WebM 使用大端序
        const durationBE = new Uint8Array(8);
        for (let j = 0; j < 8; j++) durationBE[j] = durationBytes[7 - j];

        const durationElement = new Uint8Array(10); // 2(ID) + 1(size) + 8(float64) - wait, size is VINT
        durationElement[0] = 0x44; // Duration ID high byte
        durationElement[1] = 0x89; // Duration ID low byte
        durationElement[2] = 0x88; // VINT size = 8 bytes (0x88 = 0b10001000)
        durationElement.set(durationBE, 3);
        // Actually 2+1+8 = 11 bytes... let me recalculate
        const elem = new Uint8Array(11);
        elem[0] = 0x44;
        elem[1] = 0x89;
        elem[2] = 0x88; // VINT: 8 bytes
        elem.set(durationBE, 3);

        // 重建文件：前半部分 + Duration 元素 + 后半部分
        const result = new Uint8Array(data.length + 11);
        result.set(data.subarray(0, insertPos), 0);
        result.set(elem, insertPos);
        result.set(data.subarray(insertPos), insertPos + 11);

        // 更新 Info 段的 size（增加 11 字节）
        // 读取原 size
        let origSize = 0;
        const sizeBytes = data.subarray(sizeStart, sizeStart + sizeLen);
        if (sizeLen === 1) {
          origSize = sizeBytes[0] & 0x7F;
        } else if (sizeLen === 2) {
          origSize = ((sizeBytes[0] & 0x3F) << 8) | sizeBytes[1];
        } else if (sizeLen === 3) {
          origSize = ((sizeBytes[0] & 0x1F) << 16) | (sizeBytes[1] << 8) | sizeBytes[2];
        } else if (sizeLen === 4) {
          origSize = ((sizeBytes[0] & 0x0F) << 24) | (sizeBytes[1] << 16) | (sizeBytes[2] << 8) | sizeBytes[3];
        }

        // 如果 size 是 "unknown size" (全1)，不修改
        const maxVal = (1 << (7 * sizeLen)) - 1;
        if (origSize === maxVal) {
          // Unknown size, 无法安全修改 — 直接返回带 Duration 的版本
          // 大多数播放器仍能处理
          return new Blob([result], { type: "video/webm" });
        }

        const newSize = origSize + 11;
        // 写回新 size
        if (sizeLen === 1 && newSize <= 0x7F) {
          result[sizeStart] = 0x80 | newSize;
        } else if (sizeLen === 2 && newSize <= 0x3FFF) {
          result[sizeStart] = 0x40 | (newSize >> 8);
          result[sizeStart + 1] = newSize & 0xFF;
        } else if (sizeLen === 3 && newSize <= 0x1FFFFF) {
          result[sizeStart] = 0x20 | (newSize >> 16);
          result[sizeStart + 1] = (newSize >> 8) & 0xFF;
          result[sizeStart + 2] = newSize & 0xFF;
        }
        // 4+ byte sizes 很少见，跳过

        return new Blob([result], { type: "video/webm" });
      }
    }
  } catch (e) {
    console.warn("修复 WebM Duration 失败，使用原始文件:", e);
  }
  return blob;
}

export function useRecording() {
  const isRecording = ref(false);
  const canRecord = ref(true);
  const recordings = ref<RecordingEntry[]>([]);

  let mediaRecorder: MediaRecorder | null = null;
  let recordedChunks: Blob[] = [];
  let stream: MediaStream | null = null;
  let recordingStartTime = 0;

  function loadRecordings() {
    try {
      const saved = localStorage.getItem(STORAGE_KEY);
      if (saved) recordings.value = JSON.parse(saved);
    } catch (_) { /* ignored */ }
  }

  function saveRecordingsList() {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(recordings.value));
  }

  function toggle(canvas: HTMLCanvasElement, fps = 30) {
    if (isRecording.value) {
      stop();
    } else {
      start(canvas, fps);
    }
  }

  function start(canvas: HTMLCanvasElement, fps = 30) {
    if (typeof MediaRecorder === "undefined" || typeof canvas.captureStream !== "function") {
      canRecord.value = false;
      return;
    }

    stream = canvas.captureStream(fps);
    recordedChunks = [];

    let mimeType = "video/webm;codecs=vp9";
    if (!MediaRecorder.isTypeSupported(mimeType)) {
      mimeType = "video/webm";
      if (!MediaRecorder.isTypeSupported(mimeType)) {
        canRecord.value = false;
        return;
      }
    }

    mediaRecorder = new MediaRecorder(stream, {
      mimeType,
      videoBitsPerSecond: 5_000_000,
    });
    mediaRecorder.ondataavailable = (e) => {
      if (e.data.size > 0) recordedChunks.push(e.data);
    };
    recordingStartTime = Date.now();
    mediaRecorder.onstop = async () => {
      const rawBlob = new Blob(recordedChunks, { type: "video/webm" });
      // 修复 WebM 进度条不可拖动：注入 Duration 元数据
      const blob = await fixWebmDuration(rawBlob, Date.now() - recordingStartTime);
      const id = `rec_${Date.now()}`;
      const name = new Date().toLocaleString().replace(/[/\\:]/g, "-");

      // 保存到 IndexedDB
      try {
        await saveBlob(id, blob);
        recordings.value.unshift({ id, name, date: Date.now(), size: blob.size });
        if (recordings.value.length > 20) {
          const removed = recordings.value.pop();
          if (removed) deleteBlob(removed.id).catch(() => {});
        }
        saveRecordingsList();
      } catch (_) { /* ignored */ }

      // 同时触发下载
      const url = URL.createObjectURL(blob);
      const a = document.createElement("a");
      a.href = url;
      a.download = `lan-desk-recording-${id}.webm`;
      a.click();
      setTimeout(() => URL.revokeObjectURL(url), 5000);
    };
    mediaRecorder.start(1000);
    isRecording.value = true;
  }

  function stop() {
    mediaRecorder?.stop();
    stream?.getTracks().forEach(t => t.stop());
    stream = null;
    isRecording.value = false;
  }

  async function playRecording(id: string): Promise<string | null> {
    try {
      const blob = await loadBlob(id);
      if (blob) return URL.createObjectURL(blob);
    } catch (_) { /* ignored */ }
    return null;
  }

  async function removeRecording(id: string) {
    await deleteBlob(id).catch(() => {});
    recordings.value = recordings.value.filter(r => r.id !== id);
    saveRecordingsList();
  }

  function downloadRecording(id: string) {
    loadBlob(id).then(blob => {
      if (!blob) return;
      const url = URL.createObjectURL(blob);
      const a = document.createElement("a");
      a.href = url;
      a.download = `lan-desk-recording-${id}.webm`;
      a.click();
      setTimeout(() => URL.revokeObjectURL(url), 5000);
    }).catch(() => {});
  }

  function destroy() {
    if (mediaRecorder && mediaRecorder.state !== "inactive") {
      try { mediaRecorder.stop(); } catch (_) { /* ignored */ }
    }
    mediaRecorder = null;
    stream?.getTracks().forEach(t => t.stop());
    stream = null;
  }

  // 初始化时加载历史
  loadRecordings();

  return {
    isRecording,
    canRecord,
    recordings,
    toggle,
    destroy,
    playRecording,
    removeRecording,
    downloadRecording,
  };
}
