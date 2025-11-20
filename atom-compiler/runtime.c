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

// Concatenate two strings (frees input strings)
char* __builtin_string_concat(char* a, char* b) {
    if (!a || !b) {
        // Handle NULL inputs - return the non-NULL one or NULL
        if (!a && !b) return NULL;
        if (!a) return b;
        if (!b) return a;
    }
    
    size_t len_a = strlen(a);
    size_t len_b = strlen(b);
    
    char* result = (char*)malloc(len_a + len_b + 1);
    if (!result) {
        free(a);
        free(b);
        return NULL;
    }
    
    memcpy(result, a, len_a);
    memcpy(result + len_a, b, len_b + 1);
    
    // Free the input strings since we're done with them
    free(a);
    free(b);
    
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
