using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Automation;

namespace BalanciFy.App;

public sealed partial class MainWindow : Window
{
    private readonly DispatcherTimer _refreshTimer = new() { Interval = TimeSpan.FromSeconds(2) };
    private readonly SemaphoreSlim _refreshGate = new(1, 1);
    private readonly Task _preferencesLoadTask;
    private IReadOnlyList<FlowRuleSnapshot> _rules = Array.Empty<FlowRuleSnapshot>();
    private UiPreferences? _preferences;
    private bool _hasPreferencesToRestore;
    private bool _preferencesSynchronized;
    private bool _serviceDisconnected;
    private string? _preferencesLoadWarning;
    private bool _loadingSettings;
    private bool _settingsDirty;
    private bool _rulesLoaded;
    private uint? _editingRuleId;
    private string _page = "home";

    public MainWindow()
    {
        InitializeComponent();
        _preferencesLoadTask = LoadPreferencesAsync();
        RuleClassBox.SelectionChanged += RuleClassBox_SelectionChanged;
        UpdatePinnedRuleInput();
        RootNavigation.SelectedItem = RootNavigation.MenuItems[0];
        _refreshTimer.Tick += async (_, _) => await RefreshStatusAsync();
        _refreshTimer.Start();
        Activated += async (_, _) => await RefreshStatusAsync();
    }

    private async void RefreshButton_Click(object sender, RoutedEventArgs e)
    {
        await RefreshStatusAsync();
    }

    private void RuleClassBox_SelectionChanged(object sender, SelectionChangedEventArgs e) => UpdatePinnedRuleInput();

    private void UpdatePinnedRuleInput()
    {
        RulePinnedUplinkBox.IsEnabled = RuleClassBox.SelectedItem is ComboBoxItem item &&
                                        byte.TryParse(item.Tag?.ToString(), out var ruleClass) &&
                                        ruleClass == 2;
    }

    private async Task LoadPreferencesAsync()
    {
        try
        {
            _preferences = await UiPreferencesStore.LoadAsync(CancellationToken.None);
            _hasPreferencesToRestore = _preferences is not null;
            _preferencesSynchronized = !_hasPreferencesToRestore;
        }
        catch (Exception error) when (IsPreferenceLoadFailure(error))
        {
            _preferencesSynchronized = true;
            _preferencesLoadWarning = $"Saved local preferences were ignored: {error.Message}";
        }
    }

    private static bool IsPreferenceLoadFailure(Exception error) =>
        error is IOException ||
        error is UnauthorizedAccessException ||
        error is System.Text.Json.JsonException ||
        error is InvalidDataException;

    private static bool IsSupportBundleExportFailure(Exception error) =>
        error is IOException ||
        error is UnauthorizedAccessException ||
        error is TimeoutException ||
        error is System.Runtime.InteropServices.COMException ||
        error is InvalidDataException;

    private async Task<UiPreferences> ReconcileSavedRulesAsync(UiPreferences preferences, CancellationToken cancellationToken)
    {
        var serviceRules = await IpcClient.GetFlowRulesAsync(cancellationToken);
        var serviceRulesById = serviceRules.ToDictionary(rule => rule.Id);
        var usedIds = serviceRulesById.Keys.ToHashSet();
        var reconciledRules = new List<FlowRuleSnapshot>(preferences.CustomRules.Count);
        var changed = false;
        foreach (var savedRule in preferences.CustomRules)
        {
            var rule = savedRule;
            if (serviceRulesById.TryGetValue(savedRule.Id, out var currentRule) && currentRule != savedRule)
            {
                rule = savedRule with { Id = NextCustomRuleId(usedIds) };
                usedIds.Add(rule.Id);
                changed = true;
            }
            else
            {
                usedIds.Add(rule.Id);
            }
            reconciledRules.Add(rule);
        }

        if (!changed)
        {
            return preferences;
        }

        var reconciled = preferences with { CustomRules = reconciledRules };
        await UiPreferencesStore.SaveAsync(reconciled, CancellationToken.None);
        return reconciled;
    }

    private static uint NextCustomRuleId(IEnumerable<uint> occupiedIds)
    {
        var used = occupiedIds.ToHashSet();
        for (uint id = 1_000; id < uint.MaxValue; id++)
        {
            if (!used.Contains(id))
            {
                return id;
            }
        }
        throw new InvalidOperationException("No custom rule identifiers are available.");
    }

    private async Task SynchronizePreferencesAsync(UiPreferences preferences, CancellationToken cancellationToken)
    {
        await IpcClient.SetBondModeAsync(preferences.BondEnabled, cancellationToken);
        await IpcClient.SetDuplicateModeAsync(preferences.DuplicateMode, cancellationToken);
        await IpcClient.SetAllowMeteredAsync(preferences.AllowMetered, cancellationToken);
        foreach (var rule in preferences.CustomRules)
        {
            await IpcClient.SetFlowRuleAsync(rule, cancellationToken);
        }
    }

    private UiPreferences BuildPreferences(IReadOnlyList<FlowRuleSnapshot> customRules)
    {
        var unchangedSettings = _settingsDirty ? _preferences : null;
        return new UiPreferences(
            1,
            unchangedSettings?.BondEnabled ?? BondToggle.IsOn,
            unchangedSettings?.DuplicateMode ?? DuplicateToggle.IsOn,
            unchangedSettings?.AllowMetered ?? MeteredToggle.IsOn,
            customRules.Where(rule => rule.Id >= 1_000).ToArray());
    }

    private void Settings_Toggled(object sender, RoutedEventArgs e)
    {
        if (!_loadingSettings)
        {
            _settingsDirty = true;
            ApplySettingsButton.IsEnabled = true;
            DiscardSettingsButton.IsEnabled = true;
        }
    }

    private async void DiscardSettingsButton_Click(object sender, RoutedEventArgs e)
    {
        _settingsDirty = false;
        ApplySettingsButton.IsEnabled = false;
        DiscardSettingsButton.IsEnabled = false;
        await RefreshStatusAsync();
    }

    private async void ResetPreferencesButton_Click(object sender, RoutedEventArgs e)
    {
        await _preferencesLoadTask;
        if (!await _refreshGate.WaitAsync(0))
        {
            return;
        }

        RefreshButton.IsEnabled = false;
        ResetPreferencesButton.IsEnabled = false;
        var reset = false;
        try
        {
            var dialog = new ContentDialog
            {
                XamlRoot = RootNavigation.XamlRoot,
                Title = "Reset saved preferences?",
                Content = "This removes this Windows user's saved policy and custom-rule snapshot. Current service settings remain active until the service restarts; this shell will not restore the deleted snapshot.",
                PrimaryButtonText = "Reset saved preferences",
                CloseButtonText = "Cancel",
                DefaultButton = ContentDialogButton.Close,
            };
            if (await dialog.ShowAsync() != ContentDialogResult.Primary)
            {
                return;
            }

            UiPreferencesStore.Delete();
            _preferences = new UiPreferences(
                1,
                BondToggle.IsOn,
                DuplicateToggle.IsOn,
                MeteredToggle.IsOn,
                Array.Empty<FlowRuleSnapshot>());
            _hasPreferencesToRestore = false;
            _preferencesSynchronized = true;
            _preferencesLoadWarning = null;
            _settingsDirty = false;
            ApplySettingsButton.IsEnabled = false;
            DiscardSettingsButton.IsEnabled = false;
            ConnectionInfo.Severity = InfoBarSeverity.Success;
            ConnectionInfo.Title = "Saved preferences removed";
            ConnectionInfo.Message = "The current service policy was not changed.";
            reset = true;
        }
        catch (Exception error) when (error is IOException or UnauthorizedAccessException or System.Runtime.InteropServices.COMException)
        {
            ConnectionInfo.Severity = InfoBarSeverity.Warning;
            ConnectionInfo.Title = "Could not reset saved preferences";
            ConnectionInfo.Message = error.Message;
        }
        finally
        {
            RefreshButton.IsEnabled = true;
            ResetPreferencesButton.IsEnabled = true;
            _refreshGate.Release();
        }

        if (reset)
        {
            await RefreshStatusAsync();
        }
    }

    private async void ApplySettingsButton_Click(object sender, RoutedEventArgs e)
    {
        await _preferencesLoadTask;
        if (!await _refreshGate.WaitAsync(0))
        {
            return;
        }

        RefreshButton.IsEnabled = false;
        ApplySettingsButton.IsEnabled = false;
        DiscardSettingsButton.IsEnabled = false;
        SelfTestButton.IsEnabled = false;
        BondToggle.IsEnabled = false;
        DuplicateToggle.IsEnabled = false;
        MeteredToggle.IsEnabled = false;
        var applied = false;
        try
        {
            using var timeout = new CancellationTokenSource(TimeSpan.FromSeconds(5));
            await IpcClient.SetBondModeAsync(BondToggle.IsOn, timeout.Token);
            await IpcClient.SetDuplicateModeAsync(DuplicateToggle.IsOn, timeout.Token);
            await IpcClient.SetAllowMeteredAsync(MeteredToggle.IsOn, timeout.Token);
            _settingsDirty = false;
            applied = true;
            var customRules = _preferences?.CustomRules ?? Array.Empty<FlowRuleSnapshot>();
            var savedPreferences = new UiPreferences(
                1,
                BondToggle.IsOn,
                DuplicateToggle.IsOn,
                MeteredToggle.IsOn,
                customRules);
            try
            {
                await UiPreferencesStore.SaveAsync(savedPreferences, CancellationToken.None);
                _preferences = savedPreferences;
                _hasPreferencesToRestore = true;
                _preferencesSynchronized = true;
                _preferencesLoadWarning = null;
                ConnectionInfo.Severity = InfoBarSeverity.Success;
                ConnectionInfo.Title = "Settings applied and saved";
                ConnectionInfo.Message = "The service accepted the policy settings, which are also saved for this Windows user.";
            }
            catch (Exception error) when (error is IOException or UnauthorizedAccessException)
            {
                _preferences = savedPreferences;
                _hasPreferencesToRestore = true;
                _preferencesSynchronized = true;
                ConnectionInfo.Severity = InfoBarSeverity.Warning;
                ConnectionInfo.Title = "Settings applied but not saved";
                ConnectionInfo.Message = $"The service accepted the settings, but local persistence failed: {error.Message}";
            }
        }
        catch (OperationCanceledException)
        {
            ShowDisconnected("Settings update timed out; earlier values may have applied. Your selections are retained—retry Apply or discard the changes.");
        }
        catch (Exception error) when (error is IOException or UnauthorizedAccessException or TimeoutException)
        {
            ShowDisconnected($"Settings update failed; earlier values may have applied. Your selections are retained—retry Apply or discard the changes. {error.Message}");
        }
        finally
        {
            RefreshButton.IsEnabled = true;
            SelfTestButton.IsEnabled = true;
            BondToggle.IsEnabled = true;
            DuplicateToggle.IsEnabled = true;
            MeteredToggle.IsEnabled = true;
            ApplySettingsButton.IsEnabled = _settingsDirty;
            DiscardSettingsButton.IsEnabled = _settingsDirty;
            _refreshGate.Release();
        }
        if (applied)
        {
            await RefreshStatusAsync();
        }
    }

    private async void AddRuleButton_Click(object sender, RoutedEventArgs e)
    {
        var name = RuleNameBox.Text.Trim();
        if (string.IsNullOrWhiteSpace(name))
        {
            ShowRuleValidationError("Enter a name for the rule.");
            return;
        }
        if (RuleClassBox.SelectedItem is not ComboBoxItem classItem ||
            !byte.TryParse(classItem.Tag?.ToString(), out var ruleClass) ||
            RuleProtocolBox.SelectedItem is not ComboBoxItem protocolItem ||
            !byte.TryParse(protocolItem.Tag?.ToString(), out var protocol))
        {
            ShowRuleValidationError("Choose a valid traffic class and protocol.");
            return;
        }
        if (!ushort.TryParse(RulePortStartBox.Text, out var portStart) ||
            !ushort.TryParse(RulePortEndBox.Text, out var portEnd) ||
            portStart > portEnd)
        {
            ShowRuleValidationError("Destination ports must be integers from 0 to 65535, with the first no greater than the last.");
            return;
        }
        if (!uint.TryParse(RulePriorityBox.Text, out var priority))
        {
            ShowRuleValidationError("Priority must be an unsigned 32-bit integer.");
            return;
        }
        byte pinnedUplink = 0;
        if (ruleClass == 2 && !byte.TryParse(RulePinnedUplinkBox.Text, out pinnedUplink))
        {
            ShowRuleValidationError("Pinned uplink ID must be an integer from 0 to 255.");
            return;
        }
        if (!_editingRuleId.HasValue && _rules.Count(existing => existing.Id >= 1_000) >= UiPreferencesStore.MaxCustomRules)
        {
            ShowRuleValidationError($"A maximum of {UiPreferencesStore.MaxCustomRules} custom rules can be saved for this Windows user.");
            return;
        }

        var rule = new FlowRuleSnapshot(
            _editingRuleId ?? NextCustomRuleId(),
            name,
            ruleClass,
            pinnedUplink,
            priority,
            protocol,
            portStart,
            portEnd,
            RuleDuplicateCheckBox.IsChecked == true);
        var wasEditing = _editingRuleId.HasValue;
        var action = wasEditing ? "Updated" : "Added";
        if (await RunRuleMutationAsync(
            cancellationToken => IpcClient.SetFlowRuleAsync(rule, cancellationToken),
            $"{action} rule '{name}'. The service accepted the change.",
            ruleToSave: rule))
        {
            ResetRuleEditor();
        }
    }

    private void EditRuleButton_Click(object sender, RoutedEventArgs e)
    {
        if (sender is not Button { Tag: uint id } || id < 1_000)
        {
            return;
        }
        var rule = _rules.FirstOrDefault(candidate => candidate.Id == id);
        if (rule is null || rule.Class > 2)
        {
            return;
        }

        _editingRuleId = rule.Id;
        RuleNameBox.Text = rule.Name;
        RuleClassBox.SelectedIndex = rule.Class;
        RulePinnedUplinkBox.Text = rule.PinnedUplink.ToString();
        RuleProtocolBox.SelectedItem = FindComboItem(RuleProtocolBox, rule.Protocol)
            ?? AddProtocolItem(rule.Protocol);
        RulePortStartBox.Text = rule.PortStart.ToString();
        RulePortEndBox.Text = rule.PortEnd.ToString();
        RulePriorityBox.Text = rule.Priority.ToString();
        RuleDuplicateCheckBox.IsChecked = rule.DuplicateEligible;
        AddRuleButton.Content = "Save rule";
        CancelRuleEditButton.IsEnabled = true;
        RuleNameBox.Focus(FocusState.Programmatic);
    }

    private void CancelRuleEditButton_Click(object sender, RoutedEventArgs e) => ResetRuleEditor();

    private void ResetRuleEditor()
    {
        _editingRuleId = null;
        RuleNameBox.Text = string.Empty;
        RuleClassBox.SelectedIndex = 0;
        RulePinnedUplinkBox.Text = "0";
        RuleProtocolBox.SelectedIndex = 0;
        RulePortStartBox.Text = "0";
        RulePortEndBox.Text = "0";
        RulePriorityBox.Text = "100";
        RuleDuplicateCheckBox.IsChecked = false;
        AddRuleButton.Content = "Add rule";
        CancelRuleEditButton.IsEnabled = false;
    }

    private static ComboBoxItem? FindComboItem(ComboBox combo, byte value) =>
        combo.Items.OfType<ComboBoxItem>().FirstOrDefault(item =>
            byte.TryParse(item.Tag?.ToString(), out var tag) && tag == value);

    private ComboBoxItem AddProtocolItem(byte protocol)
    {
        var item = new ComboBoxItem { Content = $"IP protocol {protocol}", Tag = protocol };
        RuleProtocolBox.Items.Add(item);
        return item;
    }

    private async void RemoveRuleButton_Click(object sender, RoutedEventArgs e)
    {
        if (sender is not Button { Tag: uint id } || id < 1_000)
        {
            return;
        }
        var rule = _rules.FirstOrDefault(candidate => candidate.Id == id);
        if (await RunRuleMutationAsync(
            cancellationToken => IpcClient.RemoveFlowRuleAsync(id, cancellationToken),
            $"Removed rule '{rule?.Name ?? id.ToString()}'. The service accepted the change.",
            removedRuleId: id) &&
            _editingRuleId == id)
        {
            ResetRuleEditor();
        }
    }

    private async Task<bool> RunRuleMutationAsync(
        Func<CancellationToken, Task> mutation,
        string successMessage,
        FlowRuleSnapshot? ruleToSave = null,
        uint? removedRuleId = null)
    {
        await _preferencesLoadTask;
        if (!await _refreshGate.WaitAsync(0))
        {
            return false;
        }

        RefreshButton.IsEnabled = false;
        ApplySettingsButton.IsEnabled = false;
        SelfTestButton.IsEnabled = false;
        AddRuleButton.IsEnabled = false;
        var applied = false;
        try
        {
            using var timeout = new CancellationTokenSource(TimeSpan.FromSeconds(5));
            await mutation(timeout.Token);
            applied = true;
            ConnectionInfo.Severity = InfoBarSeverity.Success;
            ConnectionInfo.Title = "Traffic rule updated";
            ConnectionInfo.Message = successMessage;
            var ownedRules = (_preferences?.CustomRules ?? Array.Empty<FlowRuleSnapshot>())
                .ToDictionary(rule => rule.Id);
            if (removedRuleId.HasValue)
            {
                ownedRules.Remove(removedRuleId.Value);
            }
            if (ruleToSave is not null)
            {
                ownedRules[ruleToSave.Id] = ruleToSave;
            }
            var savedPreferences = BuildPreferences(ownedRules.Values.OrderBy(rule => rule.Id).ToArray());
            try
            {
                await UiPreferencesStore.SaveAsync(savedPreferences, CancellationToken.None);
                _preferencesLoadWarning = null;
                ConnectionInfo.Message = $"{successMessage} This shell's custom rules are saved for this Windows user.";
            }
            catch (Exception error) when (error is IOException or UnauthorizedAccessException)
            {
                ConnectionInfo.Severity = InfoBarSeverity.Warning;
                ConnectionInfo.Title = "Rule applied but not saved";
                ConnectionInfo.Message = $"The service accepted the rule, but local persistence failed: {error.Message}";
            }
            _preferences = savedPreferences;
            _hasPreferencesToRestore = true;
            _preferencesSynchronized = true;
        }
        catch (OperationCanceledException)
        {
            ShowDisconnected("Rule update timed out. Refresh the rules before retrying.");
        }
        catch (Exception error) when (error is IOException or UnauthorizedAccessException or TimeoutException or ArgumentException)
        {
            ShowDisconnected($"Rule update failed: {error.Message}");
        }
        finally
        {
            RefreshButton.IsEnabled = true;
            ApplySettingsButton.IsEnabled = _settingsDirty;
            SelfTestButton.IsEnabled = true;
            AddRuleButton.IsEnabled = _rulesLoaded;
            _refreshGate.Release();
        }
        if (applied)
        {
            await RefreshStatusAsync();
        }
        return applied;
    }

    private void RenderRules()
    {
        _rulesLoaded = true;
        AddRuleButton.IsEnabled = true;
        RulesList.Children.Clear();
        if (_rules.Count == 0)
        {
            RulesList.Children.Add(new TextBlock { Text = "No traffic rules are configured." });
            return;
        }

        foreach (var rule in _rules)
        {
            var row = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 10 };
            row.Children.Add(new TextBlock
            {
                Text = FormatRule(rule),
                TextWrapping = TextWrapping.Wrap,
                VerticalAlignment = VerticalAlignment.Center,
                MaxWidth = 700,
            });
            if (rule.Id >= 1_000)
            {
                var editButton = new Button { Content = "Edit", Tag = rule.Id };
                AutomationProperties.SetName(editButton, $"Edit rule {rule.Name}");
                editButton.Click += EditRuleButton_Click;
                row.Children.Add(editButton);
                var removeButton = new Button { Content = "Remove", Tag = rule.Id };
                AutomationProperties.SetName(removeButton, $"Remove rule {rule.Name}");
                removeButton.Click += RemoveRuleButton_Click;
                row.Children.Add(removeButton);
            }
            RulesList.Children.Add(row);
        }
    }

    private uint NextCustomRuleId() => NextCustomRuleId(_rules.Select(rule => rule.Id));

    private void ShowRuleValidationError(string message)
    {
        ConnectionInfo.Severity = InfoBarSeverity.Warning;
        ConnectionInfo.Title = "Check the rule details";
        ConnectionInfo.Message = message;
    }

    private async void ExportSupportBundleButton_Click(object sender, RoutedEventArgs e)
    {
        if (!await _refreshGate.WaitAsync(0))
        {
            return;
        }

        RefreshButton.IsEnabled = false;
        SelfTestButton.IsEnabled = false;
        ExportSupportBundleButton.IsEnabled = false;
        try
        {
            using var timeout = new CancellationTokenSource(TimeSpan.FromSeconds(15));
            var bundle = await IpcClient.GetSupportBundleAsync(timeout.Token);
            var picker = new Windows.Storage.Pickers.FileSavePicker();
            picker.FileTypeChoices.Add("JSON document", new List<string> { ".json" });
            picker.SuggestedFileName = $"BalanciFy-support-{DateTime.Now:yyyyMMdd-HHmmss}.json";
            var windowHandle = WinRT.Interop.WindowNative.GetWindowHandle(this);
            WinRT.Interop.InitializeWithWindow.Initialize(picker, windowHandle);
            var file = await picker.PickSaveFileAsync();
            if (file is null)
            {
                return;
            }

            await Windows.Storage.FileIO.WriteTextAsync(file, bundle);
            ConnectionInfo.Severity = InfoBarSeverity.Success;
            ConnectionInfo.Title = "Support bundle exported";
            ConnectionInfo.Message = "The service-generated, redacted JSON bundle was saved to the location you selected.";
        }
        catch (OperationCanceledException)
        {
            ShowDisconnected("Support-bundle request timed out.");
        }
        catch (Exception error) when (IsSupportBundleExportFailure(error))
        {
            ShowDisconnected($"Could not export support bundle: {error.Message}");
        }
        finally
        {
            RefreshButton.IsEnabled = true;
            SelfTestButton.IsEnabled = true;
            ExportSupportBundleButton.IsEnabled = true;
            _refreshGate.Release();
        }
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
            SelfTestResultText.Text = result.Detail;
            SelfTestResultText.Visibility = Visibility.Visible;
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
            "relay" => "Relay health",
            "diagnostics" => "Service diagnostics",
            _ => "Product information",
        };
        PageMessage.Text = page switch
        {
            "rules" => "Built-in rules are read-only. Custom rules are saved in this Windows user's local preferences and re-applied when the shell reconnects; the service itself keeps policy in memory.",
            "relay" => "Relay failover and key-update health from the service. Relay endpoints and identities are deliberately redacted.",
            "diagnostics" => "Service counters and the capacity self-test are read from the local service. Export creates a redacted JSON bundle you can review or share from the save location you choose.",
            "about" => "BalanciFy is a Windows-first multi-WAN bonding project. This WinUI shell talks only to the local service over its ACL-protected named pipe.",
            _ => string.Empty,
        };
        RulesPanel.Visibility = page == "rules" ? Visibility.Visible : Visibility.Collapsed;
        RelayPanel.Visibility = page == "relay" ? Visibility.Visible : Visibility.Collapsed;
        SelfTestButton.Visibility = page == "diagnostics" ? Visibility.Visible : Visibility.Collapsed;
        ExportSupportBundleButton.Visibility = page == "diagnostics" ? Visibility.Visible : Visibility.Collapsed;
        DiagnosticsStatsText.Visibility = page == "diagnostics" ? Visibility.Visible : Visibility.Collapsed;
        SelfTestResultText.Visibility = page == "diagnostics" && !string.IsNullOrEmpty(SelfTestResultText.Text)
            ? Visibility.Visible
            : Visibility.Collapsed;
        await RefreshStatusAsync();
    }

    private async Task RefreshStatusAsync()
    {
        await _preferencesLoadTask;
        if (!await _refreshGate.WaitAsync(0))
        {
            return;
        }

        RefreshButton.IsEnabled = false;
        try
        {
            using var timeout = new CancellationTokenSource(TimeSpan.FromSeconds(
                _hasPreferencesToRestore && _preferences is not null && (!_preferencesSynchronized || _serviceDisconnected) ? 30 : 5));
            var snapshot = await IpcClient.GetStatusAsync(timeout.Token);
            if (_hasPreferencesToRestore && _preferences is not null && !_settingsDirty &&
                (_serviceDisconnected ||
                 snapshot.BondEnabled != _preferences.BondEnabled ||
                 snapshot.DuplicateMode != _preferences.DuplicateMode ||
                 snapshot.AllowMetered != _preferences.AllowMetered))
            {
                _preferencesSynchronized = false;
            }
            if (_hasPreferencesToRestore && _preferences is not null && !_settingsDirty && !_preferencesSynchronized)
            {
                var preferencesToRestore = await ReconcileSavedRulesAsync(_preferences, timeout.Token);
                _preferences = preferencesToRestore;
                await SynchronizePreferencesAsync(preferencesToRestore, timeout.Token);
                _preferencesSynchronized = true;
                snapshot = await IpcClient.GetStatusAsync(timeout.Token);
            }
            _serviceDisconnected = false;
            if (_preferencesLoadWarning is { } preferencesWarning)
            {
                ConnectionInfo.Severity = InfoBarSeverity.Warning;
                ConnectionInfo.Title = "Saved preferences were ignored";
                ConnectionInfo.Message = preferencesWarning;
            }
            else
            {
                ConnectionInfo.Severity = InfoBarSeverity.Success;
                ConnectionInfo.Title = "Connected to the BalanciFy service";
                ConnectionInfo.Message = $"Local named pipe · refreshed {DateTime.Now:T}";
            }
            ServiceVersionText.Text = snapshot.ServiceVersion;
            SchemaText.Text = $"IPC schema {snapshot.SchemaVersion} · active flows {snapshot.ActiveFlows}";
            _preferences ??= new UiPreferences(
                1,
                snapshot.BondEnabled,
                snapshot.DuplicateMode,
                snapshot.AllowMetered,
                Array.Empty<FlowRuleSnapshot>());
            if (!_settingsDirty)
            {
                _loadingSettings = true;
                try
                {
                    BondToggle.IsOn = snapshot.BondEnabled;
                    DuplicateToggle.IsOn = snapshot.DuplicateMode;
                    MeteredToggle.IsOn = snapshot.AllowMetered;
                }
                finally
                {
                    _loadingSettings = false;
                }
                ApplySettingsButton.IsEnabled = false;
                DiscardSettingsButton.IsEnabled = false;
            }
            PolicyText.Text = $"Policy {snapshot.PolicyRevision} · MTU {snapshot.AdapterMtu}";
            UplinkList.ItemsSource = snapshot.Uplinks
                .Select(uplink => $"Uplink {uplink.Id}: {StateName(uplink.State)} · RTT {uplink.RttMs} ms · loss {uplink.LossPermille}‰ · capacity {FormatBitsPerSecond(uplink.CapacityBps)} · {(uplink.Eligible ? "eligible" : "ineligible")}{(uplink.Metered ? " · metered" : string.Empty)}")
                .ToArray();
            if (_page == "rules")
            {
                _rules = await IpcClient.GetFlowRulesAsync(timeout.Token);
                RenderRules();
            }
            if (_page == "relay")
            {
                var relay = await IpcClient.GetRelaySnapshotAsync(timeout.Token);
                var convergence = relay.LastConvergenceMs is { } milliseconds
                    ? $"{milliseconds:N0} ms"
                    : "not yet recorded";
                RelayStatusText.Text = $"Relay configured: {(relay.Configured ? "yes" : "no")} · failovers: {relay.FailoverCount:N0} · last failover convergence: {convergence} · coordinated key update due: {(relay.RekeyUpdateRequired ? "yes" : "no")}";
            }
            if (_page == "diagnostics")
            {
                DiagnosticsStatsText.Text = $"Forwarded {snapshot.PacketsForwarded:N0} packets · refused {snapshot.Refused:N0} · authentication failures {snapshot.AuthenticationFailures:N0} · ECN CE {snapshot.EcnCe:N0} · delay signals {snapshot.DelayEvents:N0}";
            }
        }
        catch (OperationCanceledException)
        {
            ShowDisconnected("Service request or saved-preference restoration timed out. The previous snapshot is retained.");
        }
        catch (Exception error) when (error is IOException or UnauthorizedAccessException or TimeoutException or System.Text.Json.JsonException)
        {
            ShowDisconnected($"Could not refresh service state or restore saved preferences: {error.Message}");
        }
        finally
        {
            RefreshButton.IsEnabled = true;
            _refreshGate.Release();
        }
    }

    private void ShowDisconnected(string message)
    {
        _serviceDisconnected = true;
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
