# JNI entry points are resolved by name from Rust, so nothing here may be renamed.
-keep class com.nxslauncher.NxsActivity { *; }
-keepclasseswithmembernames class * {
    native <methods>;
}
-dontwarn org.jetbrains.annotations.**
