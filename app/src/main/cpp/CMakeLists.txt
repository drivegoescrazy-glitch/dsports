cmake_minimum_required(VERSION 3.22.1)
project(dsports_native LANGUAGES CXX)

add_library(dsports_native SHARED
    native_port.cpp
)

target_compile_features(dsports_native PRIVATE cxx_std_17)
target_compile_options(dsports_native PRIVATE -Wall -Wextra -Werror)
target_link_libraries(dsports_native PRIVATE android log)
