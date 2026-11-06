#!/bin/bash
set -euo pipefail
# Build only on the build host, under the workspace's shared compile.lock.
export CFLAGS="$CFLAGS -march=x86-64 -mtune=generic"
export CXXFLAGS="$CXXFLAGS -march=x86-64 -mtune=generic"
export CMAKE_PREFIX_PATH="$PREFIX"
export PKG_CONFIG_PATH="$PREFIX/lib/pkgconfig:$PREFIX/share/pkgconfig"
export CMAKE_ARGS="-DCMAKE_FIND_ROOT_PATH='$PREFIX;$BUILD_PREFIX/$CONDA_TOOLCHAIN_HOST/sysroot' -DCMAKE_FIND_ROOT_PATH_MODE_LIBRARY=ONLY -DCMAKE_FIND_ROOT_PATH_MODE_INCLUDE=ONLY -DNCBI_ThirdParty_BerkeleyDB=$PREFIX -DCMAKE_LIBRARY_PATH=$PREFIX/lib -DCMAKE_INCLUDE_PATH=$PREFIX/include -DNCBI_COMPILER_FLAGS_SSE=-msse2 -DCMAKE_EXPORT_COMPILE_COMMANDS=ON -DCMAKE_CXX_STANDARD_LIBRARIES=$PREFIX/lib/libiconv.so"
./cmake-configure --without-debug --without-dll --with-build-root=build-linux \
  --with-projects=app/table2asn --with-targets='^table2asn$' \
  --with-components='XML;XSLT;EXSLT;SQLITE3;BerkeleyDB;-LocalZCF'
cmake --build build-linux/build --target table2asn --parallel 12
mkdir -p "$PREFIX/bin" "$PREFIX/share/basecraft-table2asn"
cp build-linux/bin/table2asn "$PREFIX/bin/"
"$STRIP" "$PREFIX/bin/table2asn"
patchelf --set-rpath '$ORIGIN/../lib' "$PREFIX/bin/table2asn"
cp LICENSE "$PREFIX/share/basecraft-table2asn/NCBI-LICENSE"
cp "$RECIPE_DIR/build-lock.json" "$PREFIX/share/basecraft-table2asn/"
printf '%s\n' 'Toolkit30.8.0 source 01ab652af9da9d557b1553504d4dcd4db12f5b86' 'Tool version is measured separately; no NCBI release build number supplied.' > "$PREFIX/share/basecraft-table2asn/SOURCE.txt"
test "$("$PREFIX/bin/table2asn" -version)" = 'table2asn: 1.30.0'
