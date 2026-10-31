// GOW-Port: procedural wait to recover a caller of our own from a Windows DLL.
#include <windows.h>
volatile unsigned ticks = 0;
__declspec(noinline) void ProceduralWaitProfile()
{
    Sleep(10);
    ticks = ticks + 1; // Keep the frame; avoid turning Sleep into a final jump.
}
int main()
{
    for (unsigned i=0;i<1500;++i) ProceduralWaitProfile();
    return 0;
}
