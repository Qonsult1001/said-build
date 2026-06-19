using System.Threading.Tasks;
namespace Lib;
public class Worker {
    public int Count { get; private set; }
    // Returns Task (awaitable + observable), NOT async void.
    public async Task DoAsync(){ await Task.Yield(); Count++; }
}
