cmake_minimum_required(VERSION 3.20)
project(cmake-hello C)

add_executable(cmake-hello src/main.c)

enable_testing()
add_test(NAME cmake-hello_runs COMMAND cmake-hello)
