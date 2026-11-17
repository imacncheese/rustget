using Microsoft.Win32;
using System.Collections.ObjectModel;
using System.Diagnostics;
using System.IO;
using System.Net;
using System.Net.Http;
using System.Net.Http.Json;
using System.Net.Sockets;
using System.Text.Json;
using System.Windows;
using System.Windows.Threading;

namespace RustGet.Native;

public partial class MainWindow : Window
{
    private readonly RustGetApi _api = new();
    private readonly ObservableCollection<DownloadRow> _rows = new();
    private List<DownloadRow> _all = [];
    private string _filter = "all";
    private string _search = "";
    private bool _ready;
    private System.Windows.Forms.NotifyIcon? _tray;
    private readonly HashSet<string> _notifiedCompleted = [];
    private bool _firstRefresh = true;
    private readonly DispatcherTimer _timer = new() { Interval = TimeSpan.FromSeconds(1.5) };
    private readonly DispatcherTimer _clipboardTimer = new() { Interval = TimeSpan.FromSeconds(1) };
    private string _lastClipboard = "";

    public MainWindow()
    {
        InitializeComponent();
        ThemeManager.Apply(this, AppSettings.Current.Theme, AppSettings.Current.Accent);
        DownloadsList.ItemsSource = _rows;
        Loaded += async (_, _) =>
        {
            _ready = true;
            InitializeTray();
            try
            {
                await _api.StartAsync();
                await RefreshAsync();
                _timer.Tick += async (_, _) => await RefreshAsync();
                _timer.Start();
            }
            catch (Exception error)
            {
                DetailStatus.Text = error.Message;
            }
        };
        StateChanged += (_, _) =>
        {
            if (WindowState == WindowState.Minimized)
            {
                Hide();
                _tray?.ShowBalloonTip(1500, "RustGet", "Still running in the notification area.", System.Windows.Forms.ToolTipIcon.Info);
            }
        };
        _clipboardTimer.Tick += (_, _) => CheckClipboard();
        _clipboardTimer.Start();
        Closed += (_, _) =>
        {
            if (_tray is not null)
            {
                _tray.Visible = false;
                _tray.Dispose();
            }
            _api.Dispose();
        };
    }

    private void CheckClipboard()
    {
        if (!AppSettings.Current.MonitorClipboard)
        {
            return;
        }
        try
        {
            if (!System.Windows.Clipboard.ContainsText())
            {
                return;
            }
            var text = System.Windows.Clipboard.GetText().Trim();
            if (!(text.StartsWith("https://", StringComparison.OrdinalIgnoreCase)
                || text.StartsWith("http://", StringComparison.OrdinalIgnoreCase))
                || text == _lastClipboard)
            {
                return;
            }
            _lastClipboard = text;
            RestoreFromTray();
            var dialog = new AddDialog(text) { Owner = this };
            dialog.ShowDialog();
        }
        catch
        {
        }
    }

    private void InitializeTray()
    {
        if (_tray is not null)
        {
            return;
        }
        var icon = System.Drawing.Icon.ExtractAssociatedIcon(Environment.ProcessPath ?? "");
        _tray = new System.Windows.Forms.NotifyIcon
        {
            Text = "RustGet",
            Icon = icon,
            Visible = true,
            ContextMenuStrip = new System.Windows.Forms.ContextMenuStrip(),
        };
        _tray.ContextMenuStrip.Items.Add("Open RustGet", null, (_, _) => RestoreFromTray());
        _tray.ContextMenuStrip.Items.Add("Exit", null, (_, _) =>
        {
            _tray.Visible = false;
            Close();
        });
        _tray.DoubleClick += (_, _) => RestoreFromTray();
    }

    private void RestoreFromTray()
    {
        Show();
        ShowInTaskbar = true;
        WindowState = WindowState.Normal;
        Activate();
    }

    private async Task RefreshAsync()
    {
        try
        {
            _all = (await _api.ListAsync()).ToList();
            ApplyFilter();
        }
        catch (Exception error)
        {
            DetailStatus.Text = error.Message;
        }
    }

    private void ApplyFilter()
    {
        if (!_ready)
        {
            return;
        }
        var selected = (DownloadsList.SelectedItem as DownloadRow)?.Id;
        var rows = _all.Where(row =>
            (_filter == "all" || row.Status.Equals(_filter, StringComparison.OrdinalIgnoreCase)
                || row.Category.Equals(_filter, StringComparison.OrdinalIgnoreCase))
            && (_search.Length == 0
                || row.FileName.Contains(_search, StringComparison.OrdinalIgnoreCase)
                || row.Url.Contains(_search, StringComparison.OrdinalIgnoreCase)));
        _rows.Clear();
        foreach (var row in rows)
        {
            _rows.Add(row);
        }
        EmptyState.Visibility = _rows.Count == 0 ? Visibility.Visible : Visibility.Collapsed;
        AllCount.Text = _all.Count.ToString();
        DownloadingCount.Text = _all.Count(row => row.Status.Equals("downloading", StringComparison.OrdinalIgnoreCase)).ToString();
        QueuedCount.Text = _all.Count(row => row.Status.Equals("queued", StringComparison.OrdinalIgnoreCase)).ToString();
        PausedCount.Text = _all.Count(row => row.Status.Equals("paused", StringComparison.OrdinalIgnoreCase)).ToString();
        CompletedCount.Text = _all.Count(row => row.Status.Equals("completed", StringComparison.OrdinalIgnoreCase)).ToString();
        FailedCount.Text = _all.Count(row => row.Status.Equals("failed", StringComparison.OrdinalIgnoreCase)).ToString();
        SectionTitle.Text = _filter == "all" ? "All downloads" : _filter;
        SectionSubtitle.Text = $"{_rows.Count} item{(_rows.Count == 1 ? "" : "s")}";
        ActiveStat.Text = DownloadingCount.Text;
        CompletedStat.Text = CompletedCount.Text;
        SpeedStat.Text = FormatBytes((ulong)_all.Sum(row => (double)row.SpeedBps));
        DataStat.Text = FormatBytes((ulong)_all.Sum(row => (double)row.Downloaded));
        foreach (var row in _all.Where(row => row.Status.Equals("completed", StringComparison.OrdinalIgnoreCase)))
        {
            if (_notifiedCompleted.Add(row.Id) && !_firstRefresh && _tray is not null
                && AppSettings.Current.Notifications)
            {
                _tray.ShowBalloonTip(3000, "RustGet", $"{row.FileName} downloaded", System.Windows.Forms.ToolTipIcon.Info);
            }
        }
        _firstRefresh = false;
        if (selected is not null)
        {
            DownloadsList.SelectedItem = _rows.FirstOrDefault(row => row.Id == selected);
        }
        if (DownloadsList.SelectedItem is null)
        {
            DetailBody.Visibility = Visibility.Collapsed;
            DetailEmpty.Visibility = Visibility.Visible;
            DetailName.Text = "Select a download";
            DetailStatus.Text = "Choose an item from the list";
            DetailProgress.Value = 0;
        }
    }

    private void Filter_Changed(object sender, System.Windows.Controls.SelectionChangedEventArgs e)
    {
        if (!_ready)
        {
            return;
        }
        if (sender == FilterList)
        {
            if (FilterList.SelectedItem is System.Windows.Controls.ListBoxItem item && item.Tag is string tag)
            {
                CategoryList.SelectedItem = null;
                _filter = tag;
                ApplyFilter();
            }
        }
        else if (selectedCategory() is string category)
        {
            FilterList.SelectedItem = null;
            _filter = category;
            ApplyFilter();
        }

        string? selectedCategory()
        {
            return (CategoryList.SelectedItem as System.Windows.Controls.ListBoxItem)?.Tag as string;
        }
    }

    private void Search_Changed(object sender, System.Windows.Controls.TextChangedEventArgs e)
    {
        if (!_ready)
        {
            return;
        }
        _search = SearchBox.Text.Trim();
        SearchHint.Visibility = SearchBox.Text.Length == 0 ? Visibility.Visible : Visibility.Collapsed;
        ApplyFilter();
    }

    private void Selection_Changed(object sender, System.Windows.Controls.SelectionChangedEventArgs e)
    {
        if (DownloadsList.SelectedItem is not DownloadRow row)
        {
            DetailBody.Visibility = Visibility.Collapsed;
            DetailEmpty.Visibility = Visibility.Visible;
            DetailName.Text = "Select a download";
            DetailStatus.Text = "Choose an item from the list";
            DetailProgress.Value = 0;
            return;
        }
        DetailBody.Visibility = Visibility.Visible;
        DetailEmpty.Visibility = Visibility.Collapsed;
        DetailName.Text = row.FileName;
        DetailStatus.Text = $"{row.Status} · {row.Category}";
        DetailProgress.Value = row.Progress;
        DetailDownloaded.Text = row.SizeText;
        DetailSpeed.Text = row.SpeedText;
        DetailPath.Text = row.Path;
        DetailUrl.Text = row.Url;
    }

    public static string FormatBytes(ulong bytes)
    {
        string[] units = ["B", "KiB", "MiB", "GiB", "TiB"];
        double value = bytes;
        var unit = 0;
        while (value >= 1024 && unit < units.Length - 1)
        {
            value /= 1024;
            unit++;
        }
        return unit == 0 ? $"{bytes} B" : $"{value:F1} {units[unit]}";
    }

    private async void PauseAll_Click(object sender, RoutedEventArgs e)
    {
        await _api.PostAsync("api/v1/downloads/pause-all");
        await RefreshAsync();
    }

    private async void Delete_Click(object sender, RoutedEventArgs e)
    {
        if (DownloadsList.SelectedItem is DownloadRow row)
        {
            await _api.DeleteAsync($"api/v1/downloads/{row.Id}");
            await RefreshAsync();
        }
    }

    private async void DeleteCompleted_Click(object sender, RoutedEventArgs e)
    {
        foreach (var row in _all.Where(row => row.Status.Equals("completed", StringComparison.OrdinalIgnoreCase)).ToList())
        {
            await _api.DeleteAsync($"api/v1/downloads/{row.Id}");
        }
        await RefreshAsync();
    }

    private void Options_Click(object sender, RoutedEventArgs e)
    {
        var dialog = new OptionsDialog { Owner = this };
        dialog.ShowDialog();
    }

    private async void Scheduler_Click(object sender, RoutedEventArgs e)
    {
        var dialog = new SchedulerDialog(_api) { Owner = this };
        await dialog.LoadAsync();
        dialog.ShowDialog();
    }

    private void Find_Click(object sender, RoutedEventArgs e) => SearchBox.Focus();

    private void About_Click(object sender, RoutedEventArgs e) =>
        System.Windows.MessageBox.Show(this,
            "RustGet 0.1.0\nNative XAML download manager\nRust engine · clean-room implementation",
            "About RustGet", MessageBoxButton.OK, MessageBoxImage.Information);

    private void Theme_Click(object sender, RoutedEventArgs e)
    {
        var settings = AppSettings.Current;
        settings.Theme = settings.Theme == "Light" ? "Dark" : "Light";
        settings.Save();
        ThemeManager.Apply(this, settings.Theme, settings.Accent);
    }

    private void Minimize_Click(object sender, RoutedEventArgs e) => WindowState = WindowState.Minimized;

    private void Maximize_Click(object sender, RoutedEventArgs e) =>
        WindowState = WindowState == WindowState.Maximized ? WindowState.Normal : WindowState.Maximized;

    private void Close_Click(object sender, RoutedEventArgs e) => Close();

    private async void Add_Click(object sender, RoutedEventArgs e)
    {
        var dialog = new AddDialog { Owner = this };
        if (dialog.ShowDialog() == true && !string.IsNullOrWhiteSpace(dialog.Url))
        {
            try
            {
                await _api.AddAsync(dialog.Url.Trim());
                await RefreshAsync();
            }
            catch (Exception error)
            {
                System.Windows.MessageBox.Show(this, error.Message, "RustGet");
            }
        }
    }

    private async void Pause_Click(object sender, RoutedEventArgs e)
    {
        if (DownloadsList.SelectedItem is DownloadRow row)
        {
            await _api.PostAsync($"api/v1/downloads/{row.Id}/pause");
            await RefreshAsync();
        }
    }

    private async void Resume_Click(object sender, RoutedEventArgs e)
    {
        if (DownloadsList.SelectedItem is DownloadRow row)
        {
            await _api.PostAsync($"api/v1/downloads/{row.Id}/resume");
            await RefreshAsync();
        }
    }

    private async void OpenFolder_Click(object sender, RoutedEventArgs e)
    {
        if (DownloadsList.SelectedItem is DownloadRow row)
        {
            await _api.PostAsync($"api/v1/downloads/{row.Id}/open");
        }
    }
}

public sealed class DownloadRow
{
    public string Id { get; init; } = "";
    public string Url { get; init; } = "";
    public string FileName { get; init; } = "";
    public string Status { get; init; } = "";
    public string Category { get; init; } = "General";
    public string Path { get; init; } = "";
    public string SizeText { get; init; } = "";
    public string SpeedText { get; init; } = "";
    public string ProgressText { get; init; } = "";
    public string Subtitle { get; init; } = "";
    public ulong Downloaded { get; init; }
    public double SpeedBps { get; init; }
    public double Progress { get; init; }
}

public sealed class AddDialog : Window
{
    private readonly System.Windows.Controls.TextBox _url = new() { Margin = new Thickness(0, 10, 0, 14) };
    public string Url => _url.Text;

    public AddDialog(string? initialUrl = null)
    {
        _url.Text = initialUrl ?? "";
        _url.Style = (Style)System.Windows.Application.Current.MainWindow.FindResource(typeof(System.Windows.Controls.TextBox));
        Title = "Add download";
        Width = 540;
        Height = 220;
        WindowStyle = WindowStyle.None;
        ResizeMode = ResizeMode.NoResize;
        WindowStartupLocation = WindowStartupLocation.CenterOwner;
        Background = ThemeManager.Brush("Panel");
        Foreground = ThemeManager.Brush("Text");
        MouseLeftButtonDown += (_, _) => DragMove();
        var panel = new System.Windows.Controls.StackPanel { Margin = new Thickness(22) };
        panel.Children.Add(new System.Windows.Controls.TextBlock
        {
            Text = "Add download",
            FontSize = 20,
            FontWeight = System.Windows.FontWeights.Bold,
        });
        panel.Children.Add(new System.Windows.Controls.TextBlock
        {
            Text = "Paste an HTTP(S), FTP or media URL",
            Foreground = new System.Windows.Media.SolidColorBrush(System.Windows.Media.Color.FromRgb(0xA2, 0xAA, 0xB6)),
            Margin = new Thickness(0, 4, 0, 14),
        });
        panel.Children.Add(_url);
        var buttons = new System.Windows.Controls.StackPanel
        {
            Orientation = System.Windows.Controls.Orientation.Horizontal,
            HorizontalAlignment = System.Windows.HorizontalAlignment.Right,
            Margin = new Thickness(0, 14, 0, 0),
        };
        var cancel = new System.Windows.Controls.Button
        {
            Content = "Cancel",
            IsCancel = true,
            MinWidth = 90,
            Margin = new Thickness(0, 0, 8, 0),
            Style = (Style)System.Windows.Application.Current.MainWindow.FindResource("Action"),
        };
        var add = new System.Windows.Controls.Button
        {
            Content = "Add download",
            IsDefault = true,
            MinWidth = 120,
            Style = (Style)System.Windows.Application.Current.MainWindow.FindResource("Primary"),
        };
        add.Click += (_, _) => { DialogResult = true; };
        buttons.Children.Add(cancel);
        buttons.Children.Add(add);
        panel.Children.Add(buttons);
        Content = new System.Windows.Controls.Border
        {
            BorderBrush = new System.Windows.Media.SolidColorBrush(System.Windows.Media.Color.FromRgb(0x2C, 0x33, 0x3E)),
            BorderThickness = new Thickness(1),
            Child = panel,
        };
        Loaded += (_, _) => _url.Focus();
    }
}

public sealed class AppSettings
{
    public bool StartWithWindows { get; set; }
    public bool MonitorClipboard { get; set; } = true;
    public bool Notifications { get; set; } = true;
    public string Theme { get; set; } = "Dark";
    public string Accent { get; set; } = "Orange";

    private static readonly string FilePath = System.IO.Path.Combine(
        Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData),
        "RustGet", "Native", "settings.json");

    public static AppSettings Current { get; private set; } = Load();

    private static AppSettings Load()
    {
        try
        {
            if (System.IO.File.Exists(FilePath))
            {
                return JsonSerializer.Deserialize<AppSettings>(System.IO.File.ReadAllText(FilePath)) ?? new();
            }
        }
        catch
        {
        }
        return new();
    }

    public void Save()
    {
        try
        {
            System.IO.Directory.CreateDirectory(System.IO.Path.GetDirectoryName(FilePath)!);
            System.IO.File.WriteAllText(FilePath, JsonSerializer.Serialize(this));
            Current = this;
            ApplyStartup();
        }
        catch
        {
        }
    }

    public void ApplyStartup()
    {
        try
        {
            using var key = Registry.CurrentUser.OpenSubKey(
                @"Software\Microsoft\Windows\CurrentVersion\Run", writable: true);
            if (StartWithWindows)
            {
                key?.SetValue("RustGet", $"\"{Environment.ProcessPath}\"");
            }
            else
            {
                key?.DeleteValue("RustGet", throwOnMissingValue: false);
            }
        }
        catch
        {
        }
    }
}

public sealed class OptionsDialog : Window
{
    private readonly System.Windows.Controls.CheckBox _startup;
    private readonly System.Windows.Controls.CheckBox _clipboard;
    private readonly System.Windows.Controls.CheckBox _notifications;
    private readonly System.Windows.Controls.ComboBox _theme;
    private readonly System.Windows.Controls.ComboBox _accent;

    public OptionsDialog()
    {
        Title = "RustGet Options";
        Width = 620;
        Height = 540;
        WindowStyle = WindowStyle.None;
        ResizeMode = ResizeMode.NoResize;
        WindowStartupLocation = WindowStartupLocation.CenterOwner;
        Background = ThemeManager.Brush("Panel");
        Foreground = ThemeManager.Brush("Text");
        MouseLeftButtonDown += (_, _) => DragMove();

        _startup = Check("Launch RustGet with Windows", AppSettings.Current.StartWithWindows);
        _clipboard = Check("Monitor the clipboard for download links", AppSettings.Current.MonitorClipboard);
        _notifications = Check("Show a notification when a download completes", AppSettings.Current.Notifications);
        _theme = Combo(["Dark", "Light", "System"], AppSettings.Current.Theme);
        _accent = Combo(["Orange", "Blue", "Green", "Violet"], AppSettings.Current.Accent);
        var root = new System.Windows.Controls.StackPanel { Margin = new Thickness(24) };
        root.Children.Add(new System.Windows.Controls.TextBlock
        {
            Text = "Options",
            FontSize = 22,
            FontWeight = System.Windows.FontWeights.Bold,
        });
        root.Children.Add(new System.Windows.Controls.TextBlock
        {
            Text = "General",
            Foreground = new System.Windows.Media.SolidColorBrush(System.Windows.Media.Color.FromRgb(0xA2, 0xAA, 0xB6)),
            Margin = new Thickness(0, 4, 0, 18),
        });
        var appearance = new System.Windows.Controls.Grid { Margin = new Thickness(0, 0, 0, 14) };
        appearance.ColumnDefinitions.Add(new System.Windows.Controls.ColumnDefinition());
        appearance.ColumnDefinitions.Add(new System.Windows.Controls.ColumnDefinition());
        var themePanel = new System.Windows.Controls.StackPanel { Margin = new Thickness(0, 0, 8, 0) };
        themePanel.Children.Add(new System.Windows.Controls.TextBlock
        {
            Text = "Theme",
            Foreground = new System.Windows.Media.SolidColorBrush(System.Windows.Media.Color.FromRgb(0xA2, 0xAA, 0xB6)),
            Margin = new Thickness(0, 0, 0, 5),
        });
        themePanel.Children.Add(_theme);
        var accentPanel = new System.Windows.Controls.StackPanel { Margin = new Thickness(8, 0, 0, 0) };
        accentPanel.Children.Add(new System.Windows.Controls.TextBlock
        {
            Text = "Accent",
            Foreground = new System.Windows.Media.SolidColorBrush(System.Windows.Media.Color.FromRgb(0xA2, 0xAA, 0xB6)),
            Margin = new Thickness(0, 0, 0, 5),
        });
        accentPanel.Children.Add(_accent);
        System.Windows.Controls.Grid.SetColumn(accentPanel, 1);
        appearance.Children.Add(themePanel);
        appearance.Children.Add(accentPanel);
        root.Children.Add(appearance);
        root.Children.Add(_startup);
        root.Children.Add(_clipboard);
        root.Children.Add(_notifications);
        root.Children.Add(new System.Windows.Controls.TextBlock
        {
            Text = "Download folder",
            Foreground = new System.Windows.Media.SolidColorBrush(System.Windows.Media.Color.FromRgb(0xA2, 0xAA, 0xB6)),
            Margin = new Thickness(0, 18, 0, 5),
        });
        root.Children.Add(new System.Windows.Controls.TextBox
        {
            Text = System.IO.Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.UserProfile), "Downloads", "RustGet"),
            IsReadOnly = true,
            Style = (Style)System.Windows.Application.Current.MainWindow.FindResource(typeof(System.Windows.Controls.TextBox)),
        });
        root.Children.Add(new System.Windows.Controls.TextBlock
        {
            Text = "Proxy discovery follows Windows Internet Settings. PAC evaluation is not implemented.",
            Foreground = new System.Windows.Media.SolidColorBrush(System.Windows.Media.Color.FromRgb(0xA2, 0xAA, 0xB6)),
            Margin = new Thickness(0, 12, 0, 0),
            TextWrapping = TextWrapping.Wrap,
        });
        var buttons = new System.Windows.Controls.StackPanel
        {
            Orientation = System.Windows.Controls.Orientation.Horizontal,
            HorizontalAlignment = System.Windows.HorizontalAlignment.Right,
            Margin = new Thickness(0, 22, 0, 0),
        };
        var cancel = new System.Windows.Controls.Button
        {
            Content = "Cancel",
            IsCancel = true,
            MinWidth = 90,
            Margin = new Thickness(0, 0, 8, 0),
            Style = (Style)System.Windows.Application.Current.MainWindow.FindResource("Action"),
        };
        var save = new System.Windows.Controls.Button
        {
            Content = "Save",
            IsDefault = true,
            MinWidth = 100,
            Style = (Style)System.Windows.Application.Current.MainWindow.FindResource("Primary"),
        };
        save.Click += (_, _) =>
        {
            var settings = new AppSettings
            {
                StartWithWindows = _startup.IsChecked == true,
                MonitorClipboard = _clipboard.IsChecked == true,
                Notifications = _notifications.IsChecked == true,
                Theme = _theme.SelectedItem as string ?? "Dark",
                Accent = _accent.SelectedItem as string ?? "Orange",
            };
            settings.Save();
            ThemeManager.Apply(System.Windows.Application.Current.MainWindow, settings.Theme, settings.Accent);
            DialogResult = true;
        };
        buttons.Children.Add(cancel);
        buttons.Children.Add(save);
        root.Children.Add(buttons);
        Content = new System.Windows.Controls.Border
        {
            BorderBrush = new System.Windows.Media.SolidColorBrush(System.Windows.Media.Color.FromRgb(0x2C, 0x33, 0x3E)),
            BorderThickness = new Thickness(1),
            Child = root,
        };
    }

    private static System.Windows.Controls.ComboBox Combo(string[] items, string selected) =>
        new()
        {
            ItemsSource = items,
            SelectedItem = selected,
            FontSize = 13,
            Padding = new Thickness(8, 6, 8, 6),
        };

    private static System.Windows.Controls.CheckBox Check(string text, bool value) =>
        new()
        {
            Content = text,
            IsChecked = value,
            FontSize = 14,
            Foreground = new System.Windows.Media.SolidColorBrush(System.Windows.Media.Color.FromRgb(0xF7, 0xF5, 0xF2)),
            Margin = new Thickness(0, 7, 0, 7),
        };
}

public sealed class SchedulerDialog : Window
{
    private readonly RustGetApi _api;
    private readonly System.Windows.Controls.ListBox _list = new()
    {
        Background = new System.Windows.Media.SolidColorBrush(System.Windows.Media.Color.FromRgb(0x1A, 0x1F, 0x27)),
        Foreground = new System.Windows.Media.SolidColorBrush(System.Windows.Media.Color.FromRgb(0xF7, 0xF5, 0xF2)),
        BorderThickness = new Thickness(0),
    };

    public SchedulerDialog(RustGetApi api)
    {
        _api = api;
        Title = "Scheduler";
        Width = 640;
        Height = 440;
        WindowStyle = WindowStyle.None;
        ResizeMode = ResizeMode.NoResize;
        WindowStartupLocation = WindowStartupLocation.CenterOwner;
        Background = ThemeManager.Brush("Panel");
        Foreground = ThemeManager.Brush("Text");
        MouseLeftButtonDown += (_, _) => DragMove();
        var root = new System.Windows.Controls.StackPanel { Margin = new Thickness(24) };
        root.Children.Add(new System.Windows.Controls.TextBlock
        {
            Text = "Scheduler",
            FontSize = 22,
            FontWeight = System.Windows.FontWeights.Bold,
        });
        root.Children.Add(new System.Windows.Controls.TextBlock
        {
            Text = "Download windows configured for this engine",
            Foreground = new System.Windows.Media.SolidColorBrush(System.Windows.Media.Color.FromRgb(0xA2, 0xAA, 0xB6)),
            Margin = new Thickness(0, 4, 0, 14),
        });
        root.Children.Add(_list);
        var close = new System.Windows.Controls.Button
        {
            Content = "Close",
            IsCancel = true,
            MinWidth = 100,
            HorizontalAlignment = System.Windows.HorizontalAlignment.Right,
            Margin = new Thickness(0, 16, 0, 0),
            Style = (Style)System.Windows.Application.Current.MainWindow.FindResource("Action"),
        };
        root.Children.Add(close);
        Content = new System.Windows.Controls.Border
        {
            BorderBrush = new System.Windows.Media.SolidColorBrush(System.Windows.Media.Color.FromRgb(0x2C, 0x33, 0x3E)),
            BorderThickness = new Thickness(1),
            Child = root,
        };
    }

    public async Task LoadAsync()
    {
        var json = await _api.GetStringAsync("api/v1/schedules");
        using var document = JsonDocument.Parse(json);
        _list.Items.Clear();
        if (!document.RootElement.TryGetProperty("schedules", out var schedules) || schedules.GetArrayLength() == 0)
        {
            _list.Items.Add("No schedules configured.");
            return;
        }
        foreach (var schedule in schedules.EnumerateArray())
        {
            var name = schedule.TryGetProperty("name", out var n) ? n.GetString() : "Schedule";
            var start = schedule.TryGetProperty("start_min", out var s) && s.ValueKind == JsonValueKind.Number
                ? s.GetInt32() : -1;
            var stop = schedule.TryGetProperty("stop_min", out var e) && e.ValueKind == JsonValueKind.Number
                ? e.GetInt32() : -1;
            string Time(int minute) => minute < 0 ? "—" : $"{minute / 60:00}:{minute % 60:00}";
            _list.Items.Add($"{name}   ·   start {Time(start)}   ·   stop {Time(stop)}");
        }
    }
}

public sealed class RustGetApi : IDisposable
{
    private readonly HttpClient _http = new();
    private Process? _daemon;

    public async Task StartAsync()
    {
        var data = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "RustGet", "Native");
        var downloads = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.UserProfile), "Downloads", "RustGet");
        Directory.CreateDirectory(data);
        Directory.CreateDirectory(downloads);
        var listener = new TcpListener(IPAddress.Loopback, 0);
        listener.Start();
        var port = ((IPEndPoint)listener.LocalEndpoint).Port;
        listener.Stop();
        var exe = Path.Combine(AppContext.BaseDirectory, "rustget.exe");
        if (!File.Exists(exe))
        {
            throw new FileNotFoundException("rustget.exe must be next to the native UI", exe);
        }
        _daemon = Process.Start(new ProcessStartInfo(exe,
            $"serve --data-dir \"{data}\" --download-dir \"{downloads}\" --listen 127.0.0.1:{port}")
        {
            UseShellExecute = false,
            CreateNoWindow = true,
            WorkingDirectory = AppContext.BaseDirectory,
        }) ?? throw new InvalidOperationException("cannot start rustget daemon");
        var tokenPath = Path.Combine(data, "api-token");
        string token = "";
        for (var attempt = 0; attempt < 100; attempt++)
        {
            if (File.Exists(tokenPath))
            {
                token = (await File.ReadAllTextAsync(tokenPath)).Trim();
                if (token.Length > 0) break;
            }
            await Task.Delay(100);
        }
        if (token.Length == 0) throw new InvalidOperationException("daemon did not create an API token");
        _http.BaseAddress = new Uri($"http://127.0.0.1:{port}/");
        _http.DefaultRequestHeaders.Authorization = new System.Net.Http.Headers.AuthenticationHeaderValue("Bearer", token);
        for (var attempt = 0; attempt < 60; attempt++)
        {
            try
            {
                using var probe = await _http.GetAsync("api/v1/downloads");
                if (probe.IsSuccessStatusCode)
                {
                    return;
                }
            }
            catch (HttpRequestException)
            {
            }
            await Task.Delay(100);
        }
        throw new InvalidOperationException("RustGet engine did not become ready. Close other RustGet instances and try again.");
    }

    public async Task<IReadOnlyList<DownloadRow>> ListAsync()
    {
        var response = await _http.GetFromJsonAsync<DownloadsResponse>("api/v1/downloads");
        return (response?.Downloads ?? []).Select(item =>
        {
            var total = item.TotalSize ?? 0;
            var completed = item.Status.Equals("completed", StringComparison.OrdinalIgnoreCase);
            var progress = completed ? 100 : total > 0 ? Math.Clamp(item.Downloaded * 100.0 / total, 0, 100) : 0;
            return new DownloadRow
            {
                Id = item.Id,
                Url = item.Url,
                FileName = item.FileName,
                Status = item.Status,
                Category = item.Category,
                Path = item.Destination,
                SizeText = total > 0 ? MainWindow.FormatBytes(total) : "—",
                SpeedText = item.SpeedBps > 0 ? $"{MainWindow.FormatBytes((ulong)item.SpeedBps)}/s" : "—",
                Subtitle = $"{item.Category} · {item.Status}",
                Downloaded = item.Downloaded,
                SpeedBps = item.SpeedBps,
                Progress = progress,
                ProgressText = completed ? "100%" : total > 0 ? $"{progress:F0}%" : "",
            };
        }).ToList();
    }

    public async Task AddAsync(string url)
    {
        var response = await _http.PostAsJsonAsync("api/v1/downloads", new { url });
        response.EnsureSuccessStatusCode();
    }

    public async Task PostAsync(string path)
    {
        var response = await _http.PostAsync(path, null);
        response.EnsureSuccessStatusCode();
    }

    public async Task DeleteAsync(string path)
    {
        var response = await _http.DeleteAsync(path);
        response.EnsureSuccessStatusCode();
    }

    public async Task<string> GetStringAsync(string path) => await _http.GetStringAsync(path);

    public void Dispose()
    {
        _http.Dispose();
        try
        {
            if (_daemon is { HasExited: false })
            {
                _daemon.Kill(entireProcessTree: true);
            }
        }
        catch
        {
        }
    }

    private sealed class DownloadsResponse
    {
        public List<DownloadDto> Downloads { get; set; } = [];
    }

    private sealed class DownloadDto
    {
        public string Id { get; set; } = "";
        public string Url { get; set; } = "";
        public string FileName { get; set; } = "";
        public string Status { get; set; } = "";
        public string Category { get; set; } = "General";
        public string Destination { get; set; } = "";
        public double SpeedBps { get; set; }
        public ulong Downloaded { get; set; }
        public ulong? TotalSize { get; set; }
    }
}
