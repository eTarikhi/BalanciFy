using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;

namespace BalanciFy.App;

public sealed partial class MainWindow : Window
{
    private readonly DispatcherTimer _refreshTimer = new() { Interval = TimeSpan.FromSeconds(2) };
    private readonly SemaphoreSlim _refreshGate = new(1, 1);
    private string _page = "home";

    public MainWindow()
    {
        InitializeComponent();
        RootNavigation.SelectedItem = RootNavigation.MenuItems[0];
        _refreshTimer.Tick += async (_, _) => await RefreshStatusAsync();
        _refreshTimer.Start();
        Activated += async (_, _) => await RefreshStatusAsync();
    }

    private async void RefreshButton_Click(object sender, RoutedEventArgs e)
    {
        await RefreshStatusAsync();
    }

    private async void SelfTestButton_Click(object sender, RoutedEventArgs e)
    {
        if (!await _refreshGate.WaitAsync(0))
        {
            return;
        }

        RefreshButton.IsEnabled = false;
        SelfTestButton.IsEnabled = false;
        try
        {
            using var timeout = new CancellationTokenSource(TimeSpan.FromSeconds(5));
            var result = await IpcClient.RunSelfTestAsync(timeout.Token);
            PageMessage.Text = result.Detail;
            ConnectionInfo.Severity = result.Passed ? InfoBarSeverity.Success : InfoBarSeverity.Warning;
            ConnectionInfo.Title = result.Passed ? "Service self-test passed" : "Service self-test needs attention";
            ConnectionInfo.Message = result.Detail;
        }
        catch (OperationCanceledException)
        {
            ShowDisconnected("Service self-test timed out.");
        }
        catch (Exception error) when (error is IOException or UnauthorizedAccessException or TimeoutException)
        {
            ShowDisconnected($"Could not run service self-test: {error.Message}");
        }
        finally
        {
            RefreshButton.IsEnabled = true;
            SelfTestButton.IsEnabled = true;
            _refreshGate.Release();
        }
    }

    private async void RootNavigation_SelectionChanged(
        NavigationView sender,
        NavigationViewSelectionChangedEventArgs args)
    {
        if (args.SelectedItemContainer?.Tag is not string page)
        {
            return;
        }

        _page = page;
        PageSubtitle.Text = page switch
        {
            "home" => "Multi-WAN status",
            "rules" => "Traffic policy",
            "diagnostics" => "Service diagnostics",
            _ => "Product information",
        };
        PageMessage.Text = page switch
        {
            "rules" => "Read-only service rules are shown below. Creating and editing rules are not available in this preview.",
            "diagnostics" => "The status below is read directly from the local BalanciFy service. Detailed diagnostics and support-bundle export are under development.",
            "about" => "BalanciFy is a Windows-first multi-WAN bonding project. This WinUI shell talks only to the local service over its ACL-protected named pipe.",
            _ => string.Empty,
        };
        RulesPanel.Visibility = page == "rules" ? Visibility.Visible : Visibility.Collapsed;
        SelfTestButton.Visibility = page == "diagnostics" ? Visibility.Visible : Visibility.Collapsed;
        await RefreshStatusAsync();
    }

    private async Task RefreshStatusAsync()
    {
        if (!await _refreshGate.WaitAsync(0))
        {
            return;
        }

        RefreshButton.IsEnabled = false;
        try
        {
            using var timeout = new CancellationTokenSource(TimeSpan.FromSeconds(5));
            var snapshot = await IpcClient.GetStatusAsync(timeout.Token);
            ConnectionInfo.Severity = InfoBarSeverity.Success;
            ConnectionInfo.Title = "Connected to the BalanciFy service";
            ConnectionInfo.Message = $"Local named pipe · refreshed {DateTime.Now:T}";
            ServiceVersionText.Text = snapshot.ServiceVersion;
            SchemaText.Text = $"IPC schema {snapshot.SchemaVersion} · active flows {snapshot.ActiveFlows}";
            BondText.Text = snapshot.BondEnabled ? "Enabled" : "Disabled";
            if (snapshot.DuplicateMode)
            {
                BondText.Text += " · duplicate mode";
            }
            PolicyText.Text = $"Policy {snapshot.PolicyRevision} · MTU {snapshot.AdapterMtu} · metered links {(snapshot.AllowMetered ? "allowed" : "blocked")}";
            UplinkList.ItemsSource = snapshot.Uplinks
                .Select(uplink => $"Uplink {uplink.Id}: {StateName(uplink.State)} · RTT {uplink.RttMs} ms · loss {uplink.LossPermille}‰ · capacity {FormatBitsPerSecond(uplink.CapacityBps)} · {(uplink.Eligible ? "eligible" : "ineligible")}{(uplink.Metered ? " · metered" : string.Empty)}")
                .ToArray();
            if (_page == "rules")
            {
                var rules = await IpcClient.GetFlowRulesAsync(timeout.Token);
                RulesList.ItemsSource = rules.Select(FormatRule).ToArray();
            }
            if (_page == "diagnostics")
            {
                PageMessage.Text = $"Forwarded {snapshot.PacketsForwarded:N0} packets · refused {snapshot.Refused:N0} · authentication failures {snapshot.AuthenticationFailures:N0} · ECN CE {snapshot.EcnCe:N0} · delay signals {snapshot.DelayEvents:N0}";
            }
        }
        catch (OperationCanceledException)
        {
            ShowDisconnected("Service request timed out. The previous snapshot is retained.");
        }
        catch (Exception error) when (error is IOException or UnauthorizedAccessException or TimeoutException)
        {
            ShowDisconnected($"Could not read service status: {error.Message}");
        }
        finally
        {
            RefreshButton.IsEnabled = true;
            _refreshGate.Release();
        }
    }

    private void ShowDisconnected(string message)
    {
        ConnectionInfo.Severity = InfoBarSeverity.Warning;
        ConnectionInfo.Title = "Service unavailable";
        ConnectionInfo.Message = message;
    }

    private static string FormatRule(FlowRuleSnapshot rule)
    {
        var trafficClass = rule.Class switch
        {
            0 => "Bulk",
            1 => "Latency-sensitive",
            2 => $"Pinned to uplink {rule.PinnedUplink}",
            _ => $"Unknown class {rule.Class}",
        };
        var protocol = rule.Protocol switch
        {
            0 => "any protocol",
            1 => "ICMP",
            6 => "TCP",
            17 => "UDP",
            _ => $"IP protocol {rule.Protocol}",
        };
        var ports = (rule.PortStart, rule.PortEnd) switch
        {
            (0, 0) => "all ports",
            (var start, var end) when start == end => $"port {start}",
            _ => $"ports {rule.PortStart}–{rule.PortEnd}",
        };
        return $"#{rule.Id} · {rule.Name} · {trafficClass} · {protocol}, {ports} · priority {rule.Priority}{(rule.DuplicateEligible ? " · duplicate eligible" : string.Empty)}";
    }

    private static string StateName(byte state) => state switch
    {
        0 => "probing",
        1 => "up",
        2 => "degraded",
        3 => "down",
        4 => "recovering",
        _ => $"unknown ({state})",
    };

    private static string FormatBitsPerSecond(ulong bitsPerSecond)
    {
        if (bitsPerSecond >= 1_000_000_000)
        {
            return $"{bitsPerSecond / 1_000_000_000d:0.##} Gbit/s";
        }
        if (bitsPerSecond >= 1_000_000)
        {
            return $"{bitsPerSecond / 1_000_000d:0.##} Mbit/s";
        }
        if (bitsPerSecond >= 1_000)
        {
            return $"{bitsPerSecond / 1_000d:0.##} kbit/s";
        }
        return $"{bitsPerSecond} bit/s";
    }
}
