# JNI looks these up by name from Rust.
-keepclasseswithmembernames,includedescriptorclasses class * { native <methods>; }
-keep class app.akhripasta.Core { *; }
# Compile-time-only annotations emitted by kotlinc.
-dontwarn org.jetbrains.annotations.**
