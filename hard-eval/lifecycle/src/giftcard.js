function redeemTotal(cards, feeRate) {
  let t = 0;
  for (const c of cards) t += c.amount * c.count;
  return Math.round((t + t * feeRate) * 100) / 100;
}
module.exports = { redeemTotal };
