#include <windows.h>
#include <stdio.h>
#include <stdlib.h>

int main(void) {
    const char *greeting = getenv("HELLO_GREETING");
    printf("%s\n", greeting ? greeting : "Hello from LSW");
    printf("Running on tick %lu\n", (unsigned long)GetTickCount());
    return 0;
}
