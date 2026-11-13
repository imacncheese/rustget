using System.Runtime.InteropServices;
using System.Windows;

namespace RustGet.Native;

public partial class App : Application
{
    [DllImport("kernel32.dll")]
    private static extern IntPtr GetConsoleWindow();

    [DllImport("user32.dll")]
    private static extern bool ShowWindow(IntPtr window, int command);

    public App()
    {
        InitializeComponent();
        var console = GetConsoleWindow();
        if (console != IntPtr.Zero)
        {
            ShowWindow(console, 0);
        }
    }
}
