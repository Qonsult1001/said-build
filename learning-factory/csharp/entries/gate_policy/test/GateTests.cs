using Xunit; using Advisory.Domain;
public class GateTests {
  [Fact] public void HighSeverityBlocks(){ Assert.Equal(Decision.Block, GatePolicy.Decide(9, false, 7)); }
  [Fact] public void InconclusiveQuarantines(){ Assert.Equal(Decision.Quarantine, GatePolicy.Decide(2, true, 7)); }
  [Fact] public void CleanAllows(){ Assert.Equal(Decision.Allow, GatePolicy.Decide(3, false, 7)); }
  // the gotcha: Block takes precedence even if also inconclusive
  [Fact] public void BlockBeatsQuarantine(){ Assert.Equal(Decision.Block, GatePolicy.Decide(8, true, 7)); }
}
