package com.inkframe.app.provision

/** The 12-character code drawn on the panel. Mirrors `normalize_code` in crypto.rs. */
object SetupCode {
    const val LENGTH = 12
    private const val ALPHABET = "0123456789ABCDEFGHJKMNPQRSTVWXYZ"

    /** Canonical form, or null if [input] cannot be a code. Case, spaces and dashes are ignored; O→0, I/L→1. */
    fun normalize(input: String): String? {
        val out = StringBuilder(LENGTH)
        for (raw in input) {
            val c = when (val u = raw.uppercaseChar()) {
                ' ', '-' -> continue
                'O' -> '0'
                'I', 'L' -> '1'
                else -> u
            }
            if (c !in ALPHABET) return null
            out.append(c)
        }
        return out.toString().takeIf { it.length == LENGTH }
    }

    /** `7K3M9QXR2D4F` → `7K3M-9QXR-2D4F`, for display while typing. */
    fun group(code: String): String = code.chunked(4).joinToString("-")
}
