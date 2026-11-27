/**
 * A total is only a number when every part was observed. A missing side makes the point absent
 * (the chart breaks the line) instead of plotting a 0 the Agent never measured.
 */
export function totalOrNull(left, right) {
  if (left == null || right == null) return null;
  return Number(left) + Number(right);
}
