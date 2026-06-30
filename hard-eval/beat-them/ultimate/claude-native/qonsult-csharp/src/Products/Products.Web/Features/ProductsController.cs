using Microsoft.AspNetCore.Mvc;

// GET /products/featuredata/ and GET /products/product/ — list feature data and the catalog. Inherits
// ApiController; each action only delegates to its service.
[Route("products")]
public class ProductsController(
    IGetFeatureDataService featureData,
    IGetProductsService products) : ApiController
{
    [HttpGet("featuredata")]
    public async Task<ActionResult> GetFeatureData()
        => await featureData.GetFeatureData(new GetFeatureDataQuery()).ToActionResult();

    [HttpGet("product")]
    public async Task<ActionResult> GetProducts()
        => await products.GetProducts(new GetProductsQuery()).ToActionResult();
}
