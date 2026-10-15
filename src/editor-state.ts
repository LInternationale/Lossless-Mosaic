export function brushLabels(restoring: boolean, blocks: boolean) {
  return restoring
    ? {
        paint: "解密",
        erase: "取消解密",
        paintTitle: "选中需要解密的区块",
        eraseTitle: "取消选择，不改变图片数据",
      }
    : {
        paint: "打码",
        erase: "反打码",
        paintTitle: blocks ? "涂抹选中经过的网格" : "按像素精度涂抹",
        eraseTitle: blocks ? "移除经过的网格" : "按像素精度擦除",
      };
}
export function nextOperation(
  previous: "protect" | "restore",
  blocks: boolean,
): "protect" | "restore" {
  return previous === "restore" && blocks ? "restore" : "protect";
}
export type PreviewReport = {
  encrypted_blocks: number;
  plain_blocks: number;
  encrypted_entropy: { min: number; max: number; mean: number } | null;
  plain_entropy: { min: number; max: number; mean: number } | null;
  entropy_auc: number | null;
  missed_blocks: number;
  false_positive_blocks: number;
  uncertain_blocks: number;
  warning: boolean;
};
export function previewText(p: PreviewReport) {
  const format = (v: { min: number; max: number; mean: number } | null) =>
    v
      ? `${v.min.toFixed(2)}–${v.max.toFixed(2)}（均值 ${v.mean.toFixed(2)}）`
      : "无此类区块";
  return `加密 ${p.encrypted_blocks} 块，未加密 ${p.plain_blocks} 块\n加密块熵：${format(p.encrypted_entropy)}\n未加密块熵：${format(p.plain_entropy)}\n熵区分度（AUC）：${p.entropy_auc === null ? "无两类样本，无法比较" : (p.entropy_auc * 100).toFixed(1) + "%"}\n自动选择预计误选 ${p.false_positive_blocks} 块，漏选 ${p.missed_blocks} 块；样本不足的边缘格 ${p.uncertain_blocks} 块。`;
}
