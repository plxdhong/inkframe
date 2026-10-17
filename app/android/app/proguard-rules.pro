# kotlinx.serialization keeps generated serializers via its own consumer rules.
# BouncyCastle: only the lightweight API is used; R8 strips the provider.
-dontwarn org.bouncycastle.**
