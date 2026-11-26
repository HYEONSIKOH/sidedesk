# UniFFI 생성 코드는 JNA 가 이름으로 부른다.
# external fun 이름이 곧 C 심볼 이름이고, Structure 는 FieldOrder 와 필드 이름을, Callback 은 메서드를 리플렉션으로 찾는다.
-keep class uniffi.sidedesk_core.** { *; }

# JNA aar 에는 소비자 규칙이 없다. JNA 자신도 리플렉션과 네이티브에서 이름으로 찾는다.
-keep class com.sun.jna.** { *; }
-keep class * implements com.sun.jna.** { *; }

# JNA 는 데스크톱 전용 java.awt 를 참조하지만 안드로이드에서는 그 길을 타지 않는다.
-dontwarn java.awt.**
