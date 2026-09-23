// 分位数近似标注：后端 quantile_approx === true 表示 P50/P95/P99 由直方图合并得出。
// 字段缺失（后端未部署）按精确处理：不标注、不报错；样本数与平均值始终精确，不做此标注。

/** 任一相关响应声明近似即视为近似；只认 === true，undefined / false 均按精确处理。 */
export function quantileApprox(...responses) {
  return responses.some((response) => response?.quantile_approx === true)
}
