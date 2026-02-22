/*
 * ios_endian_compat.h
 *
 * iOS / macOS do not ship <endian.h>.  The byte-swap helpers live in
 * <libkern/OSByteOrder.h> under different names.  This shim is
 * force-included into every espeak-ng C translation unit via
 *   CMAKE_C_FLAGS=-include /path/to/ios_endian_compat.h
 */
#ifndef IOS_ENDIAN_COMPAT_H
#define IOS_ENDIAN_COMPAT_H

#include <libkern/OSByteOrder.h>

#ifndef le16toh
#  define le16toh(x) OSSwapLittleToHostInt16(x)
#endif
#ifndef le32toh
#  define le32toh(x) OSSwapLittleToHostInt32(x)
#endif
#ifndef le64toh
#  define le64toh(x) OSSwapLittleToHostInt64(x)
#endif
#ifndef be16toh
#  define be16toh(x) OSSwapBigToHostInt16(x)
#endif
#ifndef be32toh
#  define be32toh(x) OSSwapBigToHostInt32(x)
#endif
#ifndef be64toh
#  define be64toh(x) OSSwapBigToHostInt64(x)
#endif
#ifndef htole16
#  define htole16(x) OSSwapHostToLittleInt16(x)
#endif
#ifndef htole32
#  define htole32(x) OSSwapHostToLittleInt32(x)
#endif
#ifndef htobe16
#  define htobe16(x) OSSwapHostToBigInt16(x)
#endif
#ifndef htobe32
#  define htobe32(x) OSSwapHostToBigInt32(x)
#endif

#endif /* IOS_ENDIAN_COMPAT_H */
