/* The prebuilt ONNX Runtime libraries were compiled against glibc 2.38+, whose headers turn strtol
 * and its siblings into these names. Defining them here keeps the binary running on glibc 2.34+
 * (Ubuntu 22.04, Debian 12, RHEL 9) instead of requiring the glibc that built the libraries. */
#include <stdlib.h>

long __isoc23_strtol(const char *s, char **end, int base) { return strtol(s, end, base); }
long long __isoc23_strtoll(const char *s, char **end, int base) { return strtoll(s, end, base); }
unsigned long __isoc23_strtoul(const char *s, char **end, int base) { return strtoul(s, end, base); }
unsigned long long __isoc23_strtoull(const char *s, char **end, int base) { return strtoull(s, end, base); }
