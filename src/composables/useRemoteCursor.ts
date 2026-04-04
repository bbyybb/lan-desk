/**
 * 远程光标绘制 composable
 * 提供 5 种光标形状的绘制逻辑
 */
export function useRemoteCursor() {
  let remoteCursorX = 0;
  let remoteCursorY = 0;
  let remoteCursorShape = "Arrow";

  function updateCursor(x: number, y: number, shape: string) {
    remoteCursorX = x;
    remoteCursorY = y;
    remoteCursorShape = shape;
  }

  function drawRemoteCursor(ctx: CanvasRenderingContext2D, canvasWidth: number, canvasHeight: number) {
    const x = remoteCursorX * canvasWidth;
    const y = remoteCursorY * canvasHeight;

    ctx.save();

    if (remoteCursorShape === "IBeam") {
      // 文本光标：竖线
      ctx.strokeStyle = "#ff3333";
      ctx.lineWidth = 2;
      ctx.beginPath();
      ctx.moveTo(x, y - 10);
      ctx.lineTo(x, y + 10);
      ctx.moveTo(x - 4, y - 10);
      ctx.lineTo(x + 4, y - 10);
      ctx.moveTo(x - 4, y + 10);
      ctx.lineTo(x + 4, y + 10);
      ctx.stroke();
    } else if (remoteCursorShape === "Hand") {
      // 手形：实心圆 + 指向标记
      ctx.fillStyle = "#ff3333";
      ctx.beginPath();
      ctx.arc(x, y, 6, 0, Math.PI * 2);
      ctx.fill();
      ctx.strokeStyle = "#ffffff";
      ctx.lineWidth = 1;
      ctx.stroke();
    } else if (remoteCursorShape === "Crosshair") {
      // 十字准星
      ctx.strokeStyle = "#ff3333";
      ctx.lineWidth = 1;
      ctx.beginPath();
      ctx.moveTo(x - 10, y);
      ctx.lineTo(x + 10, y);
      ctx.moveTo(x, y - 10);
      ctx.lineTo(x, y + 10);
      ctx.stroke();
    } else if (remoteCursorShape === "Wait") {
      // 等待：双圈
      ctx.strokeStyle = "#ffaa00";
      ctx.lineWidth = 2;
      ctx.beginPath();
      ctx.arc(x, y, 8, 0, Math.PI * 2);
      ctx.stroke();
      ctx.beginPath();
      ctx.arc(x, y, 4, 0, Math.PI);
      ctx.stroke();
    } else if (remoteCursorShape === "ResizeNS") {
      // 垂直双向箭头
      ctx.strokeStyle = "#ff3333";
      ctx.lineWidth = 2;
      ctx.beginPath();
      ctx.moveTo(x, y - 10);
      ctx.lineTo(x, y + 10);
      ctx.moveTo(x - 4, y - 10);
      ctx.lineTo(x, y - 14);
      ctx.lineTo(x + 4, y - 10);
      ctx.moveTo(x - 4, y + 10);
      ctx.lineTo(x, y + 14);
      ctx.lineTo(x + 4, y + 10);
      ctx.stroke();
    } else if (remoteCursorShape === "ResizeEW") {
      // 水平双向箭头
      ctx.strokeStyle = "#ff3333";
      ctx.lineWidth = 2;
      ctx.beginPath();
      ctx.moveTo(x - 10, y);
      ctx.lineTo(x + 10, y);
      ctx.moveTo(x - 10, y - 4);
      ctx.lineTo(x - 14, y);
      ctx.lineTo(x - 10, y + 4);
      ctx.moveTo(x + 10, y - 4);
      ctx.lineTo(x + 14, y);
      ctx.lineTo(x + 10, y + 4);
      ctx.stroke();
    } else if (remoteCursorShape === "ResizeNESW") {
      // 东北-西南对角双向箭头
      ctx.strokeStyle = "#ff3333";
      ctx.lineWidth = 2;
      ctx.beginPath();
      ctx.moveTo(x - 8, y + 8);
      ctx.lineTo(x + 8, y - 8);
      ctx.moveTo(x + 4, y - 8);
      ctx.lineTo(x + 8, y - 8);
      ctx.lineTo(x + 8, y - 4);
      ctx.moveTo(x - 4, y + 8);
      ctx.lineTo(x - 8, y + 8);
      ctx.lineTo(x - 8, y + 4);
      ctx.stroke();
    } else if (remoteCursorShape === "ResizeNWSE") {
      // 西北-东南对角双向箭头
      ctx.strokeStyle = "#ff3333";
      ctx.lineWidth = 2;
      ctx.beginPath();
      ctx.moveTo(x - 8, y - 8);
      ctx.lineTo(x + 8, y + 8);
      ctx.moveTo(x - 4, y - 8);
      ctx.lineTo(x - 8, y - 8);
      ctx.lineTo(x - 8, y - 4);
      ctx.moveTo(x + 4, y + 8);
      ctx.lineTo(x + 8, y + 8);
      ctx.lineTo(x + 8, y + 4);
      ctx.stroke();
    } else if (remoteCursorShape === "Move") {
      // 四向箭头
      ctx.strokeStyle = "#ff3333";
      ctx.lineWidth = 2;
      ctx.beginPath();
      // 水平线
      ctx.moveTo(x - 10, y);
      ctx.lineTo(x + 10, y);
      // 垂直线
      ctx.moveTo(x, y - 10);
      ctx.lineTo(x, y + 10);
      // 四个箭头
      ctx.moveTo(x - 10, y - 3);
      ctx.lineTo(x - 13, y);
      ctx.lineTo(x - 10, y + 3);
      ctx.moveTo(x + 10, y - 3);
      ctx.lineTo(x + 13, y);
      ctx.lineTo(x + 10, y + 3);
      ctx.moveTo(x - 3, y - 10);
      ctx.lineTo(x, y - 13);
      ctx.lineTo(x + 3, y - 10);
      ctx.moveTo(x - 3, y + 10);
      ctx.lineTo(x, y + 13);
      ctx.lineTo(x + 3, y + 10);
      ctx.stroke();
    } else if (remoteCursorShape === "Help") {
      // 问号
      ctx.strokeStyle = "#ff3333";
      ctx.lineWidth = 2;
      ctx.beginPath();
      ctx.arc(x, y - 4, 6, Math.PI * 1.2, Math.PI * 2.0);
      ctx.arc(x, y - 4, 6, 0, Math.PI * 0.5);
      ctx.stroke();
      ctx.beginPath();
      ctx.moveTo(x, y + 2);
      ctx.lineTo(x, y + 4);
      ctx.stroke();
      ctx.fillStyle = "#ff3333";
      ctx.beginPath();
      ctx.arc(x, y + 8, 1.5, 0, Math.PI * 2);
      ctx.fill();
    } else if (remoteCursorShape === "NotAllowed") {
      // 禁止符号：圆圈 + 斜线
      ctx.strokeStyle = "#ff3333";
      ctx.lineWidth = 2;
      ctx.beginPath();
      ctx.arc(x, y, 8, 0, Math.PI * 2);
      ctx.stroke();
      ctx.beginPath();
      ctx.moveTo(x - 5.5, y - 5.5);
      ctx.lineTo(x + 5.5, y + 5.5);
      ctx.stroke();
    } else if (remoteCursorShape !== "Hidden") {
      // 默认箭头：圈 + 点
      ctx.strokeStyle = "#ff3333";
      ctx.lineWidth = 2;
      ctx.beginPath();
      ctx.arc(x, y, 8, 0, Math.PI * 2);
      ctx.stroke();
      ctx.fillStyle = "#ff3333";
      ctx.beginPath();
      ctx.arc(x, y, 2, 0, Math.PI * 2);
      ctx.fill();
    }

    ctx.restore();
  }

  return { updateCursor, drawRemoteCursor };
}
