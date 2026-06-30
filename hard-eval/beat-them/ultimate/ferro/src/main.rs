//! Ferro — a minimal invoicing service.
//!
//! Frontend layer: drives the orchestration services to create + list each
//! entity (Customer, Invoice, Payment) and prints the results. The three
//! entities share the same create(validate -> persist -> return) shape.

mod api;
mod models;
mod repository;
mod service;

use models::customer::NewCustomer;
use models::invoice::NewInvoice;
use models::payment::NewPayment;
use service::customer_service::CustomerService;
use service::invoice_service::InvoiceService;
use service::payment_service::PaymentService;

fn main() {
    let mut customers = CustomerService::new();
    let mut invoices = InvoiceService::new();
    let mut payments = PaymentService::new();

    // --- Customers ---
    let acme = api::customer_api::create(
        &mut customers,
        NewCustomer { name: "Acme Corp".into(), email: "billing@acme.test".into() },
    )
    .expect("create customer");
    api::customer_api::create(
        &mut customers,
        NewCustomer { name: "Globex".into(), email: "ap@globex.test".into() },
    )
    .expect("create customer");

    println!("Customers:");
    for c in api::customer_api::list(&customers).unwrap() {
        println!("  #{} {} <{}>", c.id, c.name, c.email);
    }

    // --- Invoices ---
    let inv = api::invoice_api::create(
        &mut invoices,
        NewInvoice { customer_id: acme.id, amount_cents: 12_500, memo: "Consulting".into() },
    )
    .expect("create invoice");
    api::invoice_api::create(
        &mut invoices,
        NewInvoice { customer_id: acme.id, amount_cents: 4_000, memo: "Hosting".into() },
    )
    .expect("create invoice");

    println!("Invoices:");
    for i in api::invoice_api::list(&invoices).unwrap() {
        println!("  #{} customer={} {}c — {}", i.id, i.customer_id, i.amount_cents, i.memo);
    }

    // --- Payments ---
    api::payment_api::create(
        &mut payments,
        NewPayment { invoice_id: inv.id, amount_cents: 12_500, method: "card".into() },
    )
    .expect("create payment");

    println!("Payments:");
    for p in api::payment_api::list(&payments).unwrap() {
        println!("  #{} invoice={} {}c via {}", p.id, p.invoice_id, p.amount_cents, p.method);
    }

    // Spot-check get_by_id on one entity.
    match api::invoice_api::get(&invoices, inv.id) {
        Ok(found) => println!("Fetched invoice #{}: {}", found.id, found.memo),
        Err(e) => eprintln!("lookup failed: {e}"),
    }
}
