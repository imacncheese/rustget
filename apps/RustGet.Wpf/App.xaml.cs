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
        DispatcherUnhandledException += (_, args) =>
        {
            try
            {
                var directory = System.IO.Path.Combine(
                    Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData),
                    "RustGet", "Native");
                System.IO.Directory.CreateDirectory(directory);
                System.IO.File.WriteAllText(
                    System.IO.Path.Combine(directory, "native-error.log"),
                    $"{DateTime.Now:O}\n{args.Exception}");
            }
            catch
            {
            }
            MessageBox.Show(args.Exception.Message, "RustGet", MessageBoxButton.OK, MessageBoxImage.Error);
            args.Handled = true;
        };
        var console = GetConsoleWindow();
        if (console != IntPtr.Zero)
        {
            ShowWindow(console, 0);
        }
    }
}
