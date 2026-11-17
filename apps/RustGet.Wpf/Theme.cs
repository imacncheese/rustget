using System.Windows;
using System.Windows.Media;
using Microsoft.Win32;

namespace RustGet.Native;

public static class ThemeManager
{
    public static string CurrentTheme { get; private set; } = "Dark";
    public static string CurrentAccent { get; private set; } = "Orange";

    public static void Apply(Window window, string theme, string accent)
    {
        CurrentTheme = theme;
        CurrentAccent = accent;
        var light = theme switch
        {
            "Light" => true,
            "System" => SystemUsesLight(),
            _ => false,
        };
        foreach (var (key, color) in Palette(light, accent))
        {
            var brush = new SolidColorBrush(
                (System.Windows.Media.Color)System.Windows.Media.ColorConverter.ConvertFromString(color));
            window.Resources[key] = brush;
            System.Windows.Application.Current.Resources[key] = brush;
        }
    }

    public static System.Windows.Media.Brush Brush(string key) =>
        (System.Windows.Media.Brush)System.Windows.Application.Current.MainWindow.FindResource(key);

    private static bool SystemUsesLight()
    {
        try
        {
            using var key = Registry.CurrentUser.OpenSubKey(
                @"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize");
            return key?.GetValue("AppsUseLightTheme") is int value && value != 0;
        }
        catch
        {
            return false;
        }
    }

    private static Dictionary<string, string> Palette(bool light, string accent)
    {
        var dark = new Dictionary<string, string>
        {
            ["Bg"] = "#0C0E12",
            ["Sidebar"] = "#11141A",
            ["TitleBar"] = "#0F1218",
            ["Panel"] = "#171B22",
            ["Panel2"] = "#1D222B",
            ["Line"] = "#2C333E",
            ["Text"] = "#F7F5F2",
            ["Muted"] = "#A2AAB6",
            ["SubtleText"] = "#C7CEDA",
            ["Input"] = "#1A1F27",
            ["Selection"] = "#33231A",
            ["PillDefault"] = "#252B35",
            ["PillCompleted"] = "#123529",
            ["PillDownloading"] = "#122B45",
            ["PillPaused"] = "#3A2E12",
            ["PillFailed"] = "#3B1719",
            ["Green"] = "#5EE0A0",
            ["Blue"] = "#6EA8FE",
            ["Yellow"] = "#F5C451",
            ["Red"] = "#FF7373",
        };
        var bright = new Dictionary<string, string>
        {
            ["Bg"] = "#F3F5F8",
            ["Sidebar"] = "#FFFFFF",
            ["TitleBar"] = "#FFFFFF",
            ["Panel"] = "#FFFFFF",
            ["Panel2"] = "#EEF2F7",
            ["Line"] = "#D5DCE5",
            ["Text"] = "#17202B",
            ["Muted"] = "#5F6B7A",
            ["SubtleText"] = "#3E4A5A",
            ["Input"] = "#F7F9FC",
            ["Selection"] = "#FDE7D7",
            ["PillDefault"] = "#EDF1F6",
            ["PillCompleted"] = "#E8F5EF",
            ["PillDownloading"] = "#E4EEFC",
            ["PillPaused"] = "#FFF4D6",
            ["PillFailed"] = "#FDE7E7",
            ["Green"] = "#0C7A50",
            ["Blue"] = "#1D64C8",
            ["Yellow"] = "#A66A00",
            ["Red"] = "#B4232A",
        };
        var palette = light ? bright : dark;
        var (color, text) = accent switch
        {
            "Blue" => (light ? "#1F6FEB" : "#6EA8FE", "#FFFFFF"),
            "Green" => (light ? "#0E8A5F" : "#5EE0A0", light ? "#FFFFFF" : "#10231A"),
            "Violet" => (light ? "#6D4AFF" : "#B49CFF", "#FFFFFF"),
            _ => (light ? "#D95F16" : "#FF8A4C", light ? "#FFFFFF" : "#261207"),
        };
        palette["Orange"] = color;
        palette["AccentText"] = text;
        return palette;
    }
}
