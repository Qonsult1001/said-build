// Products.Web — controller. Backs /products/product/ and /products/featuredata/.

using Microsoft.AspNetCore.Mvc;

public class ProductsController(
    ICreateProductService createProduct,
    IListProductsService listProducts,
    IGetFeatureDataService getFeatureData) : ApiController
{
    [HttpPost]
    public async Task<ActionResult> Product(CreateProductCommand command)
        => await createProduct.Create(command).ToActionResult();

    [HttpGet]
    public async Task<ActionResult> Product([FromQuery] bool featuredOnly = false)
        => await listProducts.List(new ListProductsQuery(featuredOnly)).ToActionResult();

    [HttpGet]
    public async Task<ActionResult> FeatureData([FromQuery] string sku)
        => await getFeatureData.GetFeatureData(new GetFeatureDataQuery(sku)).ToActionResult();
}
