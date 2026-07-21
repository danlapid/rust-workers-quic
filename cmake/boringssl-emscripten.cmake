# BoringSSL has no WebAssembly assembly implementation. Its CMake project does
# not infer this when cross-compiling, so select the portable C implementation.
set(OPENSSL_NO_ASM ON CACHE BOOL "" FORCE)

if(NOT DEFINED ENV{EMSCRIPTEN})
  message(FATAL_ERROR "EMSCRIPTEN must point to the Emscripten source tree")
endif()

include("$ENV{EMSCRIPTEN}/cmake/Modules/Platform/Emscripten.cmake")
