using System.Text.Json;

namespace BalanciFy.App;

internal sealed record UiPreferences(
    int Version,
    bool BondEnabled,
    bool DuplicateMode,
    bool AllowMetered,
    IReadOnlyList<FlowRuleSnapshot> CustomRules);

internal static class UiPreferencesStore
{
    private const int MaxFileBytes = 256 * 1024;
    internal const int MaxCustomRules = 128;
    private static readonly JsonSerializerOptions JsonOptions = new() { WriteIndented = true };

    private static string PreferencesPath
    {
        get
        {
            var localAppData = Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData);
            if (!Path.IsPathFullyQualified(localAppData))
            {
                throw new IOException("The current user's LocalAppData path is not absolute.");
            }
            return Path.Combine(localAppData, "BalanciFy", "ui-preferences.json");
        }
    }

    public static async Task<UiPreferences?> LoadAsync(CancellationToken cancellationToken)
    {
        var path = PreferencesPath;
        if (!File.Exists(path))
        {
            return null;
        }

        await using var stream = new FileStream(path, FileMode.Open, FileAccess.Read, FileShare.Read, 4096, FileOptions.Asynchronous | FileOptions.SequentialScan);
        var bytes = new byte[MaxFileBytes + 1];
        var length = 0;
        while (length < bytes.Length)
        {
            var count = await stream.ReadAsync(bytes.AsMemory(length), cancellationToken).ConfigureAwait(false);
            if (count == 0)
            {
                break;
            }
            length += count;
        }
        if (length > MaxFileBytes)
        {
            throw new InvalidDataException("Saved UI preferences exceed the size limit.");
        }

        var preferences = JsonSerializer.Deserialize<UiPreferences>(bytes.AsSpan(0, length), JsonOptions);
        if (preferences is null || preferences.Version != 1 || preferences.CustomRules is null || preferences.CustomRules.Count > MaxCustomRules ||
            preferences.CustomRules.Any(rule => !IsValidRule(rule)) ||
            preferences.CustomRules.Select(rule => rule.Id).Distinct().Count() != preferences.CustomRules.Count)
        {
            throw new InvalidDataException("Saved UI preferences contain invalid custom traffic rules.");
        }

        return preferences;
    }

    public static async Task SaveAsync(UiPreferences preferences, CancellationToken cancellationToken)
    {
        if (preferences.Version != 1 || preferences.CustomRules is null ||
            preferences.CustomRules.Count > MaxCustomRules ||
            preferences.CustomRules.Any(rule => !IsValidRule(rule)) ||
            preferences.CustomRules.Select(rule => rule.Id).Distinct().Count() != preferences.CustomRules.Count)
        {
            throw new InvalidDataException("Refusing to save invalid custom traffic rules.");
        }

        var bytes = JsonSerializer.SerializeToUtf8Bytes(preferences, JsonOptions);
        if (bytes.Length > MaxFileBytes)
        {
            throw new InvalidDataException("Saved UI preferences exceed the size limit.");
        }

        var path = PreferencesPath;
        var directory = Path.GetDirectoryName(path)!;
        Directory.CreateDirectory(directory);
        var temporaryPath = Path.Combine(directory, $"ui-preferences.{Guid.NewGuid():N}.tmp");
        try
        {
            await using (var stream = new FileStream(temporaryPath, FileMode.CreateNew, FileAccess.Write, FileShare.None, 4096, FileOptions.Asynchronous | FileOptions.WriteThrough))
            {
                await stream.WriteAsync(bytes, cancellationToken).ConfigureAwait(false);
                await stream.FlushAsync(cancellationToken).ConfigureAwait(false);
                stream.Flush(flushToDisk: true);
            }
            File.Move(temporaryPath, path, overwrite: true);
        }
        finally
        {
            try
            {
                if (File.Exists(temporaryPath))
                {
                    File.Delete(temporaryPath);
                }
            }
            catch (IOException)
            {
                // Keep the original write/replace result; a stale uniquely named temp can be cleaned up later.
            }
            catch (UnauthorizedAccessException)
            {
                // Keep the original write/replace result; a stale uniquely named temp can be cleaned up later.
            }
        }
    }

    private static bool IsValidRule(FlowRuleSnapshot? rule) =>
        rule is not null &&
        rule.Id >= 1_000 &&
        !string.IsNullOrWhiteSpace(rule.Name) &&
        rule.Name.Length <= 120 &&
        rule.Class <= 2 &&
        rule.PortStart <= rule.PortEnd;
}
