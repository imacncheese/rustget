using System.Windows;

namespace RustGet.Native;

public partial class App : System.Windows.Application
{
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
            System.Windows.MessageBox.Show(args.Exception.Message, "RustGet", MessageBoxButton.OK, MessageBoxImage.Error);
            args.Handled = true;
        };
    }
}
