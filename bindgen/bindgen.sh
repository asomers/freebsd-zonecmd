#! /bin/sh

CRATEDIR=`dirname $0`/..
FFI_RS=ffi64.rs

case `uname -m` in
i386)
	FFI_RS=ffi32.rs
	;;
esac

cat > src/${FFI_RS} << HERE
#![allow(non_camel_case_types)]
#![allow(non_upper_case_globals)]
#![allow(unused)]
#![allow(missing_docs)]
HERE

case ${FFI_RS} in
ffi32.rs)
	CLANG_ARGS=-m32
	;;
*)
	CLANG_ARGS=
	;;
esac

if [ -n "${CLANG_ARGS}" ]; then
	bindgen --allowlist-type 'disk_zone_.*' \
		--allowlist-item 'DISK_ZONE_.*' \
		${CRATEDIR}/bindgen/wrapper.h -- ${CLANG_ARGS} \
		>> ${CRATEDIR}/src/${FFI_RS}
else
	bindgen --allowlist-type 'disk_zone_.*' \
		--allowlist-item 'DISK_ZONE_.*' \
		${CRATEDIR}/bindgen/wrapper.h \
		>> ${CRATEDIR}/src/${FFI_RS}
fi
rustfmt ${CRATEDIR}/src/${FFI_RS}
