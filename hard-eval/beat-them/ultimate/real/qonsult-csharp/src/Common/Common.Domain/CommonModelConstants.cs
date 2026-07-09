// Common.Domain — shared validation limits referenced by FluentValidation validators.

public static class CommonModelConstants
{
    public static class Common
    {
        public const int MaxNameLength = 200;
        public const int MaxEmailLength = 256;
        public const double MinLatitude = -90.0;
        public const double MaxLatitude = 90.0;
        public const double MinLongitude = -180.0;
        public const double MaxLongitude = 180.0;
    }
}
