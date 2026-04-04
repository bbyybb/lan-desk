/**
 * 白板标注 composable
 *
 * 管理标注绘制状态，使用离屏 Canvas 缓存标注层，
 * 支持自由绘制、文字标注和清除标注。
 */

import { ref } from "vue";

interface Point {
  x: number;
  y: number;
}

interface AnnotationLine {
  type: "line";
  color: string;
  width: number;
  points: Point[];
}

interface AnnotationText {
  type: "text";
  text: string;
  position: Point;
  color: string;
  fontSize: number;
}

type AnnotationItem = AnnotationLine | AnnotationText;

export function useAnnotation() {
  const isAnnotating = ref(false);
  const annotationColor = ref("#ff3333");
  const lineWidth = ref(3);
  const annotationTool = ref<"pen" | "text">("pen");
  const fontSize = ref(18);

  let isDrawing = false;
  let currentPoints: Point[] = [];
  let annotations: AnnotationItem[] = [];
  let offscreenCanvas: OffscreenCanvas | null = null;
  let offscreenCtx: OffscreenCanvasRenderingContext2D | null = null;

  function startStroke(point: Point) {
    if (annotationTool.value === "text") return;
    isDrawing = true;
    currentPoints = [point];
  }

  function addPoint(point: Point) {
    if (!isDrawing) return;
    currentPoints.push(point);
  }

  function endStroke() {
    if (isDrawing && currentPoints.length > 1) {
      annotations.push({ type: "line", color: annotationColor.value, width: lineWidth.value, points: [...currentPoints] });
    }
    isDrawing = false;
    currentPoints = [];
  }

  function addText(text: string, position: Point) {
    if (!text.trim()) return;
    annotations.push({
      type: "text",
      text: text.trim(),
      position,
      color: annotationColor.value,
      fontSize: fontSize.value,
    });
    invalidateCache();
  }

  function drawAll(ctx: CanvasRenderingContext2D, canvasWidth: number, canvasHeight: number) {
    if (annotations.length === 0 && !isDrawing) return;

    if (!offscreenCanvas || offscreenCanvas.width !== canvasWidth || offscreenCanvas.height !== canvasHeight) {
      if (typeof OffscreenCanvas === "undefined") return;
      offscreenCanvas = new OffscreenCanvas(canvasWidth, canvasHeight);
      offscreenCtx = offscreenCanvas.getContext("2d") as OffscreenCanvasRenderingContext2D;
    }
    if (!offscreenCtx) return;

    offscreenCtx.clearRect(0, 0, canvasWidth, canvasHeight);
    for (const ann of annotations) {
      if (ann.type === "line") {
        drawLine(offscreenCtx, ann.color, ann.width, ann.points, canvasWidth, canvasHeight);
      } else if (ann.type === "text") {
        drawText(offscreenCtx, ann, canvasWidth, canvasHeight);
      }
    }
    if (isDrawing && currentPoints.length > 1) {
      drawLine(offscreenCtx, annotationColor.value, lineWidth.value, currentPoints, canvasWidth, canvasHeight);
    }
    ctx.drawImage(offscreenCanvas, 0, 0);
  }

  function drawLine(
    target: CanvasRenderingContext2D | OffscreenCanvasRenderingContext2D,
    color: string,
    width: number,
    points: Point[],
    w: number,
    h: number
  ) {
    if (points.length < 2) return;
    target.save();
    target.strokeStyle = color;
    target.lineWidth = width;
    target.lineCap = "round";
    target.lineJoin = "round";
    target.beginPath();
    target.moveTo(points[0].x * w, points[0].y * h);
    for (let i = 1; i < points.length; i++) {
      target.lineTo(points[i].x * w, points[i].y * h);
    }
    target.stroke();
    target.restore();
  }

  function drawText(
    target: CanvasRenderingContext2D | OffscreenCanvasRenderingContext2D,
    ann: AnnotationText,
    w: number,
    h: number
  ) {
    target.save();
    target.fillStyle = ann.color;
    target.font = `bold ${ann.fontSize}px sans-serif`;
    target.textBaseline = "top";
    target.fillText(ann.text, ann.position.x * w, ann.position.y * h);
    target.restore();
  }

  function invalidateCache() {
    offscreenCanvas = null;
    offscreenCtx = null;
  }

  function clear() {
    annotations = [];
    currentPoints = [];
    invalidateCache();
  }

  function undo() {
    if (annotations.length > 0) {
      annotations.pop();
      invalidateCache();
    }
  }

  function hasContent(): boolean {
    return annotations.length > 0 || isDrawing;
  }

  return {
    isAnnotating,
    annotationColor,
    lineWidth,
    annotationTool,
    fontSize,
    startStroke,
    addPoint,
    endStroke,
    addText,
    drawAll,
    clear,
    undo,
    hasContent,
    get isDrawing() { return isDrawing; },
  };
}
