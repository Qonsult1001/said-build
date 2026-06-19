using Xunit; using Lib;
public class ArchT {
  [Fact] public void DefaultsToClean(){ Assert.Equal(Arch.Clean, ArchChooser.Choose(false,false,false)); }
  [Fact] public void RichDomainDdd(){ Assert.Equal(Arch.Ddd, ArchChooser.Choose(true,false,false)); }
  [Fact] public void ManyFeaturesSlice(){ Assert.Equal(Arch.VerticalSlice, ArchChooser.Choose(false,true,false)); }
  [Fact] public void LegacyLayered(){ Assert.Equal(Arch.Layered, ArchChooser.Choose(false,false,true)); }
  [Fact] public void RichDomainBeatsFeatures(){ Assert.Equal(Arch.Ddd, ArchChooser.Choose(true,true,false)); }
}
