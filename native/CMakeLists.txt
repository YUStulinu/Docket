# Docket's native code: page detection and clean-up (OpenCV), OCR (Tesseract), and on Android the
# local language model (llama.cpp) and the JNI bridge.
#
# Desktop:  cmake -S native -B native/build -DCMAKE_PREFIX_PATH="<tesseract prefix>;<OpenCV dir>"
# Android:  built by the app's Gradle (externalNativeBuild), see android/app/build.gradle.kts.
cmake_minimum_required(VERSION 3.22)
project(docket_native LANGUAGES C CXX)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_POSITION_INDEPENDENT_CODE ON)

if(ANDROID)
  # Built by scripts/fetch-deps.sh android and scripts/build-deps.sh android <abi>.
  set(DOCKET_THIRD_PARTY "${CMAKE_CURRENT_SOURCE_DIR}/third_party" CACHE PATH "")
  set(DOCKET_PREFIX "${DOCKET_THIRD_PARTY}/install/android-${ANDROID_ABI}")
  # The NDK toolchain only searches its sysroot for packages; our prefix is added as a root.
  list(APPEND CMAKE_FIND_ROOT_PATH "${DOCKET_PREFIX}")
  list(APPEND CMAKE_PREFIX_PATH "${DOCKET_PREFIX}")
  set(OpenCV_DIR "${DOCKET_THIRD_PARTY}/opencv-android/sdk/native/jni")
  set(OpenCV_STATIC ON)
  find_package(CpuFeaturesNdkCompat REQUIRED CONFIG)
endif()
find_package(OpenCV REQUIRED COMPONENTS core imgproc imgcodecs)
find_package(Tesseract 5 REQUIRED CONFIG)

add_library(docket_scan STATIC src/scan.cpp src/ocr.cpp src/json.cpp)
# The installed Tesseract target carries no include directory; its config sets the variable.
target_include_directories(docket_scan PUBLIC src ${Tesseract_INCLUDE_DIRS})
target_link_libraries(docket_scan PUBLIC opencv_core opencv_imgproc opencv_imgcodecs Tesseract::libtesseract)
if(MSVC)
  target_compile_options(docket_scan PRIVATE /W4 /utf-8 /permissive-)
else()
  target_compile_options(docket_scan PRIVATE -Wall -Wextra)
endif()

if(ANDROID)
  # llama.cpp, CPU only (ARM NEON / dotprod), as part of the app's native library.
  set(LLAMA_BUILD_COMMON ON CACHE BOOL "" FORCE)
  set(LLAMA_BUILD_TESTS OFF CACHE BOOL "" FORCE)
  set(LLAMA_BUILD_EXAMPLES OFF CACHE BOOL "" FORCE)
  set(LLAMA_BUILD_SERVER OFF CACHE BOOL "" FORCE)
  set(LLAMA_BUILD_TOOLS OFF CACHE BOOL "" FORCE)
  set(LLAMA_CURL OFF CACHE BOOL "" FORCE)
  set(LLAMA_OPENSSL OFF CACHE BOOL "" FORCE)
  set(LLAMA_BUILD_MTMD OFF CACHE BOOL "" FORCE)
  set(GGML_OPENMP OFF CACHE BOOL "" FORCE)
  if(ANDROID_ABI STREQUAL "arm64-v8a")
    # The int8 dot product instructions (every phone core since 2018: Cortex-A55/A75 and later):
    # several times faster on quantized weights than plain ARMv8.0.
    set(GGML_CPU_ARM_ARCH "armv8.2-a+dotprod+fp16" CACHE STRING "" FORCE)
  endif()
  set(BUILD_SHARED_LIBS OFF CACHE BOOL "" FORCE)
  add_subdirectory(${DOCKET_THIRD_PARTY}/llama.cpp ${CMAKE_BINARY_DIR}/llama.cpp EXCLUDE_FROM_ALL)
  add_library(docket_native SHARED src/jni.cpp src/llm.cpp)
  target_link_libraries(docket_native PRIVATE docket_scan llama llama-common android log)
  # Unused code out, symbols hidden but the JNI entry points.
  target_link_options(docket_native PRIVATE -Wl,--gc-sections -Wl,--exclude-libs,ALL)
else()
  add_executable(docket-ocr tools/docket_ocr.cpp)
  target_link_libraries(docket-ocr PRIVATE docket_scan)

  enable_testing()
  add_executable(native_tests tests/native_tests.cpp)
  target_link_libraries(native_tests PRIVATE docket_scan)
  target_compile_definitions(native_tests PRIVATE
    DOCKET_TESTDATA="${CMAKE_CURRENT_SOURCE_DIR}/../testdata/synthetic"
    DOCKET_TESSDATA="${CMAKE_CURRENT_SOURCE_DIR}/third_party/tessdata")
  add_test(NAME native_tests COMMAND native_tests)
endif()

# On Windows the OpenCV DLL goes next to the programs, so they run without PATH changes.
if(WIN32 AND NOT ANDROID)
  foreach(t docket-ocr native_tests)
    add_custom_command(TARGET ${t} POST_BUILD
      COMMAND ${CMAKE_COMMAND} -E copy_if_different $<TARGET_RUNTIME_DLLS:${t}> $<TARGET_FILE_DIR:${t}>
      COMMAND_EXPAND_LISTS)
  endforeach()
endif()
