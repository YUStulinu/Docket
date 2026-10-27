# UniFFI bindings call into Rust through JNA, which finds classes and fields by name.
-keep class com.sun.jna.** { *; }
-keep class * implements com.sun.jna.** { *; }
-keep class uniffi.docket_core.** { *; }
-dontwarn java.awt.**
# The JNI functions of the native library are bound by name.
-keep class io.github.yustulinu.docket.scan.NativeScan { native <methods>; }
