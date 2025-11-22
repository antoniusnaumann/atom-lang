#include <stdio.h>
#include <stdlib.h>
#include <stdint.h>
#include <string.h>

// Runtime functions for as_string() builtin

// ========================================================================
// Signed Integer Conversions
// ========================================================================

char* __atom_int64_to_string(int64_t value) {
    char buffer[32];  // Max length for int64_t: 20 digits + sign + null
    int len = snprintf(buffer, sizeof(buffer), "%lld", (long long)value);
    if (len < 0) return NULL;
    
    char* result = (char*)malloc(len + 1);
    if (!result) return NULL;
    memcpy(result, buffer, len + 1);
    return result;
}

char* __atom_int32_to_string(int32_t value) {
    char buffer[16];  // Max length for int32_t: 11 digits + sign + null
    int len = snprintf(buffer, sizeof(buffer), "%d", value);
    if (len < 0) return NULL;
    
    char* result = (char*)malloc(len + 1);
    if (!result) return NULL;
    memcpy(result, buffer, len + 1);
    return result;
}

char* __atom_int16_to_string(int16_t value) {
    char buffer[8];  // Max length for int16_t: 6 digits + sign + null
    int len = snprintf(buffer, sizeof(buffer), "%d", (int)value);
    if (len < 0) return NULL;
    
    char* result = (char*)malloc(len + 1);
    if (!result) return NULL;
    memcpy(result, buffer, len + 1);
    return result;
}

char* __atom_int8_to_string(int8_t value) {
    char buffer[8];  // Max length for int8_t: 4 digits + sign + null
    int len = snprintf(buffer, sizeof(buffer), "%d", (int)value);
    if (len < 0) return NULL;
    
    char* result = (char*)malloc(len + 1);
    if (!result) return NULL;
    memcpy(result, buffer, len + 1);
    return result;
}

// ========================================================================
// Unsigned Integer Conversions
// ========================================================================

char* __atom_uint64_to_string(uint64_t value) {
    char buffer[32];  // Max length for uint64_t: 20 digits + null
    int len = snprintf(buffer, sizeof(buffer), "%llu", (unsigned long long)value);
    if (len < 0) return NULL;
    
    char* result = (char*)malloc(len + 1);
    if (!result) return NULL;
    memcpy(result, buffer, len + 1);
    return result;
}

char* __atom_uint32_to_string(uint32_t value) {
    char buffer[16];  // Max length for uint32_t: 10 digits + null
    int len = snprintf(buffer, sizeof(buffer), "%u", value);
    if (len < 0) return NULL;
    
    char* result = (char*)malloc(len + 1);
    if (!result) return NULL;
    memcpy(result, buffer, len + 1);
    return result;
}

char* __atom_uint16_to_string(uint16_t value) {
    char buffer[8];  // Max length for uint16_t: 5 digits + null
    int len = snprintf(buffer, sizeof(buffer), "%u", (unsigned int)value);
    if (len < 0) return NULL;
    
    char* result = (char*)malloc(len + 1);
    if (!result) return NULL;
    memcpy(result, buffer, len + 1);
    return result;
}

char* __atom_uint8_to_string(uint8_t value) {
    char buffer[8];  // Max length for uint8_t: 3 digits + null
    int len = snprintf(buffer, sizeof(buffer), "%u", (unsigned int)value);
    if (len < 0) return NULL;
    
    char* result = (char*)malloc(len + 1);
    if (!result) return NULL;
    memcpy(result, buffer, len + 1);
    return result;
}

// ========================================================================
// Floating Point Conversions
// ========================================================================

char* __atom_float64_to_string(double value) {
    char buffer[64];  // Sufficient for most double representations
    int len = snprintf(buffer, sizeof(buffer), "%g", value);
    if (len < 0) return NULL;
    
    char* result = (char*)malloc(len + 1);
    if (!result) return NULL;
    memcpy(result, buffer, len + 1);
    return result;
}

char* __atom_float32_to_string(float value) {
    char buffer[64];  // Sufficient for most float representations
    int len = snprintf(buffer, sizeof(buffer), "%g", value);
    if (len < 0) return NULL;
    
    char* result = (char*)malloc(len + 1);
    if (!result) return NULL;
    memcpy(result, buffer, len + 1);
    return result;
}

// ========================================================================
// Boolean Conversion
// ========================================================================

char* __atom_bool_to_string(uint8_t value) {
    const char* str = value ? "True" : "False";
    size_t len = strlen(str);
    
    char* result = (char*)malloc(len + 1);
    if (!result) return NULL;
    memcpy(result, str, len + 1);
    return result;
}

// ========================================================================
// Rune (Unicode Codepoint) Conversion
// ========================================================================

char* __atom_rune_to_string(int32_t value) {
    // Format as 'c' for single character, or '\uXXXX' for others
    char buffer[16];
    int len;
    
    if (value >= 32 && value < 127 && value != '\'') {
        // Printable ASCII (except single quote)
        len = snprintf(buffer, sizeof(buffer), "'%c'", (char)value);
    } else if (value == '\'') {
        len = snprintf(buffer, sizeof(buffer), "'\\''");
    } else if (value == '\n') {
        len = snprintf(buffer, sizeof(buffer), "'\\n'");
    } else if (value == '\t') {
        len = snprintf(buffer, sizeof(buffer), "'\\t'");
    } else if (value == '\r') {
        len = snprintf(buffer, sizeof(buffer), "'\\r'");
    } else if (value == '\\') {
        len = snprintf(buffer, sizeof(buffer), "'\\\\'");
    } else {
        // Use Unicode escape
        len = snprintf(buffer, sizeof(buffer), "'\\u%04X'", (unsigned int)value);
    }
    
    if (len < 0) return NULL;
    
    char* result = (char*)malloc(len + 1);
    if (!result) return NULL;
    memcpy(result, buffer, len + 1);
    return result;
}

// ========================================================================
// Legacy compatibility functions (keep for backwards compatibility)
// ========================================================================

// Convert int to string (legacy)
char* __builtin_int_to_string(int64_t value) {
    return __atom_int64_to_string(value);
}

// Convert float to string (legacy)
char* __builtin_float_to_string(double value) {
    return __atom_float64_to_string(value);
}

// Convert bool to string (legacy)
char* __builtin_bool_to_string(int8_t value) {
    return __atom_bool_to_string((uint8_t)value);
}

// Convert rune to string (legacy)
char* __builtin_rune_to_string(int32_t value) {
    return __atom_rune_to_string(value);
}

// Append a rune (UTF-8 codepoint) to a string (for concatenation)
// This is different from __builtin_rune_to_string which formats for display
char* __builtin_append_rune_to_string(char* str, int32_t rune) {
    fprintf(stderr, "[DEBUG] __builtin_append_rune_to_string: str=%p, rune=%d (0x%X '%c')\n", 
            (void*)str, rune, rune, (rune >= 32 && rune < 127) ? (char)rune : '?');
    
    if (!str) {
        str = (char*)malloc(1);
        if (!str) return NULL;
        str[0] = '\0';
    }
    
    size_t str_len = strlen(str);
    char utf8_buf[5];  // Max 4 bytes for UTF-8 + null terminator
    int utf8_len;
    
    // Encode rune as UTF-8
    if (rune < 0x80) {
        // 1-byte sequence (ASCII)
        utf8_buf[0] = (char)rune;
        utf8_len = 1;
    } else if (rune < 0x800) {
        // 2-byte sequence
        utf8_buf[0] = (char)(0xC0 | (rune >> 6));
        utf8_buf[1] = (char)(0x80 | (rune & 0x3F));
        utf8_len = 2;
    } else if (rune < 0x10000) {
        // 3-byte sequence
        utf8_buf[0] = (char)(0xE0 | (rune >> 12));
        utf8_buf[1] = (char)(0x80 | ((rune >> 6) & 0x3F));
        utf8_buf[2] = (char)(0x80 | (rune & 0x3F));
        utf8_len = 3;
    } else if (rune < 0x110000) {
        // 4-byte sequence
        utf8_buf[0] = (char)(0xF0 | (rune >> 18));
        utf8_buf[1] = (char)(0x80 | ((rune >> 12) & 0x3F));
        utf8_buf[2] = (char)(0x80 | ((rune >> 6) & 0x3F));
        utf8_buf[3] = (char)(0x80 | (rune & 0x3F));
        utf8_len = 4;
    } else {
        // Invalid codepoint, use replacement character U+FFFD
        utf8_buf[0] = (char)0xEF;
        utf8_buf[1] = (char)0xBF;
        utf8_buf[2] = (char)0xBD;
        utf8_len = 3;
    }
    utf8_buf[utf8_len] = '\0';
    
    // Allocate new string
    char* result = (char*)malloc(str_len + utf8_len + 1);
    if (!result) {
        free(str);
        return NULL;
    }
    
    // Copy original string and append UTF-8 bytes
    memcpy(result, str, str_len);
    memcpy(result + str_len, utf8_buf, utf8_len + 1);
    
    // Free original string
    free(str);
    
    fprintf(stderr, "[DEBUG] __builtin_append_rune_to_string: returning %p ('%s', len=%zu)\n", (void*)result, result, strlen(result));
    
    return result;
}

// Concatenate two strings (frees input strings)
// Concatenate two strings (does NOT free input strings to avoid crashes)
// NOTE: This will cause memory leaks. A proper fix requires:
// 1. Reference counting, OR
// 2. Ownership tracking in the compiler, OR  
// 3. Explicit memory management
char* __builtin_string_concat(char* a, char* b) {
    if (!a || !b) {
        // Handle NULL inputs
        if (!a && !b) return NULL;
        // Return a copy of the non-NULL string (don't return the original)
        if (!a) {
            size_t len = strlen(b);
            char* result = (char*)malloc(len + 1);
            if (result) memcpy(result, b, len + 1);
            return result;
        }
        if (!b) {
            size_t len = strlen(a);
            char* result = (char*)malloc(len + 1);
            if (result) memcpy(result, a, len + 1);
            return result;
        }
    }
    
    size_t len_a = strlen(a);
    size_t len_b = strlen(b);
    
    char* result = (char*)malloc(len_a + len_b + 1);
    if (!result) {
        return NULL;
    }
    
    memcpy(result, a, len_a);
    memcpy(result + len_a, b, len_b + 1);
    
    // DO NOT FREE inputs - this prevents crashes from freeing static data
    // or use-after-free bugs, at the cost of memory leaks
    
    return result;
}

// Create a string literal (allocates a copy)
char* __builtin_string_literal(const char* str) {
    if (!str) {
        return NULL;
    }
    
    size_t len = strlen(str);
    char* result = (char*)malloc(len + 1);
    if (!result) {
        return NULL;
    }
    
    memcpy(result, str, len + 1);
    return result;
}

// Print a string and then free it (to avoid memory leaks)
int __builtin_printf_and_free(char* str) {
    if (!str) {
        return 0;
    }
    
    int result = printf("%s", str);
    free(str);
    return result;
}

// ========================================================================
// Math Classification Functions
// ========================================================================

// These are wrappers around the standard C99 macros to provide
// callable functions for the compiler backend

#include <math.h>

int __atom_isnan(double x) {
    return isnan(x);
}

int __atom_isinf(double x) {
    return isinf(x);
}

int __atom_isfinite(double x) {
    return isfinite(x);
}

// Float32 variants (C macros are polymorphic, so these work too)
int __atom_isnan_f32(float x) {
    return isnan(x);
}

int __atom_isinf_f32(float x) {
    return isinf(x);
}

int __atom_isfinite_f32(float x) {
    return isfinite(x);
}

