cmake_minimum_required(VERSION 3.16)
project(gvpb LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
if(NOT CMAKE_BUILD_TYPE)
  set(CMAKE_BUILD_TYPE Release)
endif()

find_package(Protobuf REQUIRED)

file(GLOB PROTO_FILES CONFIGURE_DEPENDS "${CMAKE_CURRENT_SOURCE_DIR}/proto/*.proto")
message(STATUS "protoc: ${Protobuf_PROTOC_EXECUTABLE} ${Protobuf_VERSION}")
message(STATUS "proto files: ${PROTO_FILES}")

set(GEN_DIR "${CMAKE_CURRENT_BINARY_DIR}/gen")
file(MAKE_DIRECTORY "${GEN_DIR}")

set(PB_SRCS)
set(PB_HDRS)
foreach(p ${PROTO_FILES})
  get_filename_component(n "${p}" NAME_WE)
  add_custom_command(
    OUTPUT "${GEN_DIR}/${n}.pb.cc" "${GEN_DIR}/${n}.pb.h"
    COMMAND "${Protobuf_PROTOC_EXECUTABLE}"
            --cpp_out="${GEN_DIR}"
            -I "${CMAKE_CURRENT_SOURCE_DIR}/proto"
            "${p}"
    DEPENDS "${p}"
    COMMENT "protoc ${n}.proto"
    VERBATIM)
  list(APPEND PB_SRCS "${GEN_DIR}/${n}.pb.cc")
  list(APPEND PB_HDRS "${GEN_DIR}/${n}.pb.h")
endforeach()

add_library(gvpb STATIC ${PB_SRCS})
target_include_directories(gvpb PUBLIC "${GEN_DIR}" "${Protobuf_INCLUDE_DIRS}")
target_link_libraries(gvpb PUBLIC ${Protobuf_LIBRARIES})

add_executable(roundtrip_test src/roundtrip_test.cc)
target_link_libraries(roundtrip_test PRIVATE gvpb)
add_dependencies(roundtrip_test gvpb)

enable_testing()
add_test(NAME roundtrip COMMAND roundtrip_test)
