using System;
using Ledgerly.Api;
using Ledgerly.Models;
using Ledgerly.Repositories;
using Ledgerly.Services;

namespace Ledgerly;

/// <summary>
/// Frontend stub: wires the in-memory repos + services + controllers,
/// then creates and lists each entity to exercise the full stack.
/// </summary>
public static class Program
{
    public static void Main()
    {
        var invoices = new InvoiceController(new InvoiceService(new InvoiceRepository()));
        var customers = new CustomerController(new CustomerService(new CustomerRepository()));
        var payments = new PaymentController(new PaymentService(new PaymentRepository()));

        var customer = customers.Create(new CreateCustomerRequest("Acme Corp", "billing@acme.test"));
        var invoice = invoices.Create(new CreateInvoiceRequest(customer.Name, 1499.00m, DateTime.UtcNow.AddDays(30)));
        payments.Create(new CreatePaymentRequest(invoice.Id, 1499.00m, "card"));

        Console.WriteLine($"Customers: {customers.List().Count}");
        Console.WriteLine($"Invoices:  {invoices.List().Count}");
        Console.WriteLine($"Payments:  {payments.List().Count}");
    }
}
