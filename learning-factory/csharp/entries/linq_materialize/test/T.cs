using System.Collections.Generic; using System.Linq; using Xunit; using Lib;
public class LinqT {
  [Fact] public void DoublesEvens(){ Assert.Equal(new[]{4,8}, Pipe.EvensDoubled(new[]{1,2,3,4}).ToArray()); }
  [Fact] public void MaterializedOnce(){
    int calls=0; IEnumerable<int> Src(){ calls++; yield return 2; yield return 4; }
    var r=Pipe.EvensDoubled(Src()); var a=r.Count; var b=r.Count;   // two reads
    Assert.Equal(1, calls);   // source enumerated exactly once (materialized), not per-read
    Assert.Equal(2, a); Assert.Equal(2, b);
  }
}
