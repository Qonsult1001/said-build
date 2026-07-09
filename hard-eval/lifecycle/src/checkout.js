// Feature 1: order checkout total.
function orderTotal(items, taxRate) {
  let t = 0;
  for (const it of items) t += Math.round(it.price * it.qty * 100) / 100; // per-line round (the trap)
  return t + t * taxRate;
}
module.exports = { orderTotal };
