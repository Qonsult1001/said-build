// Coverage.Web — controller. Backs /coverage/userglobalcoverages/.

using Microsoft.AspNetCore.Mvc;

public class UserGlobalCoveragesController(
    IGetUserGlobalCoveragesService getCoverages,
    IAddCoverageAreaService addArea) : ApiController
{
    [HttpGet("{userId}")]
    public async Task<ActionResult> Get(Guid userId)
        => await getCoverages.GetCoverages(new GetUserGlobalCoveragesQuery(userId)).ToActionResult();

    [HttpPost]
    public async Task<ActionResult> AddArea(AddCoverageAreaCommand command)
        => await addArea.AddArea(command).ToActionResult();
}
