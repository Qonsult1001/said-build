namespace Advisory.Domain;
public enum Decision { Allow, Quarantine, Block }
public static class GatePolicy { public static Decision Decide(int maxSeverity, bool requiredSourceInconclusive, int blockThreshold) { if (maxSeverity >= blockThreshold) return Decision.Block; if (requiredSourceInconclusive) return Decision.Quarantine; return Decision.Allow; } }
