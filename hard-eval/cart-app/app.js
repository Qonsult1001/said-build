// Interactive cart UI — wires the DOM to cart.js. Plain browser JS, no framework.
(function () {
  const PRODUCTS = [
    { id: 'kbd', name: 'Mechanical Keyboard', price: 89.99 },
    { id: 'mse', name: 'Wireless Mouse', price: 24.5 },
    { id: 'pad', name: 'Desk Mat', price: 0.333 },   // odd price to exercise round-at-end
    { id: 'cam', name: 'HD Webcam', price: 59.95 },
  ];
  const COUPONS = { SAVE10: 10, HALF: 50, FREE: 150 }; // FREE>100% to show the clamp
  const TAX = 0.10;
  const fmt = (n) => '$' + (Math.round(n * 100) / 100).toFixed(2);

  const cart = { items: [], coupons: [] };
  const $ = (id) => document.getElementById(id);

  function renderProducts() {
    $('products').innerHTML = PRODUCTS.map(p => `
      <div class="product">
        <div class="name">${p.name}</div>
        <div class="price">${fmt(p.price)}</div>
        <button data-add="${p.id}">Add to cart</button>
      </div>`).join('');
  }

  function lineFor(id) { return cart.items.find(i => i.id === id); }

  function addItem(id) {
    const p = PRODUCTS.find(x => x.id === id);
    const line = lineFor(id);
    if (line) line.qty = setQty(line.qty + 1);   // uses cart.js clamp
    else cart.items.push({ id, name: p.name, price: p.price, qty: 1 });
    render();
  }
  function changeQty(id, qty) {
    const line = lineFor(id); if (!line) return;
    line.qty = setQty(qty);                       // cart.js clamp (neg->0, frac->floor)
    cart.items = cart.items.filter(i => i.qty > 0);
    render();
  }
  function removeItem(id) { cart.items = cart.items.filter(i => i.id !== id); render(); }

  function applyCoupon() {
    const code = ($('coupon-code').value || '').trim().toUpperCase();
    if (!(code in COUPONS)) { $('coupon-msg').textContent = 'Unknown code'; $('coupon-msg').style.color = '#ff8080'; return; }
    const before = cart.coupons.length;
    addCoupon(cart.coupons, { code, percent: COUPONS[code] }); // cart.js idempotency
    $('coupon-msg').style.color = '';
    $('coupon-msg').textContent = cart.coupons.length === before ? `${code} already applied` : `${code} applied (-${COUPONS[code]}%)`;
    render();
  }

  function render() {
    $('cart-lines').innerHTML = cart.items.length ? cart.items.map(it => `
      <li>
        <span class="nm">${it.name}</span>
        <span class="qty">
          <button data-dec="${it.id}">−</button>
          <input data-qty="${it.id}" value="${it.qty}" />
          <button data-inc="${it.id}">+</button>
        </span>
        <span>${fmt(it.price * it.qty)}</span>
        <button class="remove" data-rm="${it.id}">×</button>
      </li>`).join('') : '<li style="color:var(--muted)">Cart is empty</li>';

    const r = checkout(cart, TAX);               // cart.js: round-at-end, clamps, idempotent
    $('subtotal').textContent = fmt(r.subtotal);
    $('discount').textContent = '-' + fmt(r.discount);
    $('tax').textContent = fmt(r.tax);
    $('total').textContent = fmt(r.total);
  }

  document.addEventListener('click', (e) => {
    const t = e.target;
    if (t.dataset.add) addItem(t.dataset.add);
    else if (t.dataset.inc) changeQty(t.dataset.inc, lineFor(t.dataset.inc).qty + 1);
    else if (t.dataset.dec) changeQty(t.dataset.dec, lineFor(t.dataset.dec).qty - 1);
    else if (t.dataset.rm) removeItem(t.dataset.rm);
    else if (t.id === 'apply-coupon') applyCoupon();
    else if (t.id === 'checkout') {
      const r = checkout(cart, TAX);
      $('receipt').textContent = cart.items.length
        ? `✓ Order placed\nSubtotal ${fmt(r.subtotal)}  Discount -${fmt(r.discount)}  Tax ${fmt(r.tax)}\nCharged ${fmt(r.total)}`
        : 'Add something to the cart first.';
    }
  });
  document.addEventListener('change', (e) => {
    if (e.target.dataset.qty) changeQty(e.target.dataset.qty, parseFloat(e.target.value));
  });

  renderProducts();
  render();
})();
