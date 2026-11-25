# Keep Tauri v2 personal mobile plugins & annotated commands/callbacks under R8 full mode.
-keep class moe.dabney.personal.** { *; }
-keepclassmembers class * extends app.tauri.plugin.Plugin {
    @app.tauri.annotation.Command <methods>;
    @app.tauri.annotation.ActivityCallback <methods>;
}

# Keep Google ML Kit ComponentDiscovery registrars & Android AICore / GenAI Prompt bindings.
-keep class com.google.mlkit.** { *; }
-keep class com.google.firebase.components.** { *; }
-keep class * implements com.google.firebase.components.ComponentRegistrar {
    public <init>();
}
-keep class com.google.android.apps.aicore.** { *; }
-keep class com.google.ai.edge.aicore.** { *; }
-keep class androidx.health.connect.** { *; }
