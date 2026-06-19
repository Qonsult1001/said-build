using System; using Xunit; using Lib;
public class DddT {
  [Fact] public void NormalizesAndEquals(){ Assert.Equal(new Email("A@B.com"), new Email("a@b.com")); }
  [Fact] public void RejectsInvalid(){ Assert.Throws<ArgumentException>(()=> new Email("nope")); }
}
