using System.Collections.Generic; using System.Linq;
namespace Lib;
public static class Pipe {
    // Return a STABLE materialized snapshot so repeated reads + side-effect-free counting are consistent.
    public static IReadOnlyList<int> EvensDoubled(IEnumerable<int> src)
        => src.Where(x => x % 2 == 0).Select(x => x * 2).ToList();
}
