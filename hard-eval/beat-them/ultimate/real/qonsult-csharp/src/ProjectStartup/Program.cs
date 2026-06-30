// ProjectStartup — composition root (host only). References each context's Infrastructure + Web
// and wires DI. No business logic.

var builder = WebApplication.CreateBuilder(args);
var configuration = builder.Configuration;

builder.Services.AddControllers();
builder.Services.AddEndpointsApiExplorer();

// Accounts context
builder.Services.AddAccountsApplication(configuration);
builder.Services.AddAccountsInfrastructure(configuration);

// Coverage context
builder.Services.AddCoverageApplication(configuration);
builder.Services.AddCoverageInfrastructure(configuration);

// Search context
builder.Services.AddSearchApplication(configuration);
builder.Services.AddSearchInfrastructure(configuration);

// Products context
builder.Services.AddProductsApplication(configuration);
builder.Services.AddProductsInfrastructure(configuration);

var app = builder.Build();

app.MapControllers();

app.Run();
