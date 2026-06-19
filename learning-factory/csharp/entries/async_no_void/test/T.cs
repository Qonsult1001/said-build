using System.Threading.Tasks; using Xunit; using Lib;
public class AsyncT {
  [Fact] public async Task AwaitableCompletes(){ var w=new Worker(); await w.DoAsync(); Assert.Equal(1,w.Count); }
  [Fact] public void ReturnsTaskNotVoid(){ var m=typeof(Worker).GetMethod("DoAsync")!; Assert.Equal(typeof(Task), m.ReturnType); }
}
