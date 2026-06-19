namespace Lib;
// Default architecture guidance for a generic C# project when none is specified.
public enum Arch { Clean, VerticalSlice, Ddd, Layered }
public static class ArchChooser {
    // Choose the architecture for a task given optional signals.
    //  - no signal / unsure            -> Clean (the safe default)
    //  - many simple CRUD features     -> VerticalSlice
    //  - rich domain rules/invariants  -> Ddd
    //  - explicit legacy/n-tier ask    -> Layered
    public static Arch Choose(bool richDomainRules, bool manySimpleFeatures, bool legacyNTier)
    {
        if (legacyNTier) return Arch.Layered;
        if (richDomainRules) return Arch.Ddd;
        if (manySimpleFeatures) return Arch.VerticalSlice;
        return Arch.Clean; // DEFAULT
    }
}
