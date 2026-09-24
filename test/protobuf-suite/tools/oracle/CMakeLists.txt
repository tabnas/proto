# Builds `oracle`: protoc's own .proto parser as a command-line tool, from a
# protobuf source checkout at the release tag the corpus was extracted from.
#
#   git clone --depth 1 --branch v<release> \
#     https://github.com/protocolbuffers/protobuf.git <protobuf>
#   cmake -S . -B <build> -G Ninja -DCMAKE_BUILD_TYPE=Release \
#     -DPROTOBUF_SOURCE_DIR=<protobuf>
#   ninja -C <build> oracle
#
# CMake fetches the Abseil release that protobuf names. Ninja prints a
# [done/total] line per compile step.
cmake_minimum_required(VERSION 3.16)
project(protoc_parser_oracle CXX)
set(CMAKE_CXX_STANDARD 17)
set(PROTOBUF_SOURCE_DIR "" CACHE PATH "protobuf source checkout at the release tag")
if(NOT EXISTS "${PROTOBUF_SOURCE_DIR}/CMakeLists.txt")
  message(FATAL_ERROR "set -DPROTOBUF_SOURCE_DIR to a protobuf checkout")
endif()
set(protobuf_BUILD_TESTS OFF CACHE BOOL "" FORCE)
set(protobuf_BUILD_CONFORMANCE OFF CACHE BOOL "" FORCE)
set(protobuf_BUILD_EXAMPLES OFF CACHE BOOL "" FORCE)
set(protobuf_BUILD_PROTOC_BINARIES OFF CACHE BOOL "" FORCE)
set(protobuf_BUILD_LIBUPB OFF CACHE BOOL "" FORCE)
set(protobuf_INSTALL OFF CACHE BOOL "" FORCE)
set(ABSL_PROPAGATE_CXX_STD ON CACHE BOOL "" FORCE)
add_subdirectory("${PROTOBUF_SOURCE_DIR}" protobuf EXCLUDE_FROM_ALL)
add_executable(oracle driver.cc)
target_link_libraries(oracle PRIVATE protobuf::libprotobuf)
