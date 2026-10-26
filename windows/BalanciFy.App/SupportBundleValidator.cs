using System.Text;
using System.Text.Json;

namespace BalanciFy.App;

/// <summary>
/// Fail-closed validator for the redacted IPC support bundle schema. The
/// service owns collection/redaction; the shell only exports payloads that
/// match the allow-listed v1 shape and size bounds.
/// </summary>
internal static class SupportBundleValidator
{
    private const int MaxUtf8Bytes = 1 << 20;
    private const int MaxUplinks = 256;
    private const int MaxWarnings = 64;
    private const int MaxWarningChars = 2048;

    private static readonly string[] RootFields =
    [
        "timestamp", "service_version", "schema_version", "policy_revision",
        "active_flows", "pending_probes", "sessions", "held_packets",
        "relay_configured", "relay_failover_count", "relay_failover_convergence_ms",
        "uplink_failover_count", "uplink_failover_convergence_ms", "counters",
        "uplinks", "rekey", "congestion", "warnings",
    ];

    private static readonly string[] CounterFields =
    [
        "packets_stripped", "packets_forwarded", "duplicates_sent", "reinjections",
        "delivered", "out_of_order", "refused", "held", "path_changes", "resumes",
        "auth_failed", "keepalives", "probes", "sessions_evicted", "ecn_ce", "delay_events",
    ];

    private static readonly string[] UplinkFields =
    [
        "id", "state", "rtt_ms", "loss_permille", "capacity_bps", "weight_permille",
        "metered", "eligible", "cwnd", "in_flight",
    ];

    private static readonly string[] RekeyFields =
    ["packets_since_rekey", "last_rekey_ago_secs", "update_required"];

    private static readonly string[] CongestionFields =
    ["loss_events", "ecn_events", "delay_events", "paths", "per_path"];

    private static readonly string[] PathFields =
    ["id", "cwnd", "in_flight", "ecn_ce", "delay_events", "rtt_min_ms", "rtt_last_ms"];

    public static void Validate(string json)
    {
        ArgumentNullException.ThrowIfNull(json);
        if (json.Length == 0 || Encoding.UTF8.GetByteCount(json) > MaxUtf8Bytes)
        {
            throw new InvalidDataException("Support bundle is empty or exceeds the export size limit.");
        }

        using var document = JsonDocument.Parse(json, new JsonDocumentOptions { MaxDepth = 16 });
        var root = document.RootElement;
        RequireObject(root, RootFields, "support bundle");
        RequireString(root, "timestamp", 128);
        RequireString(root, "service_version", 64);
        RequireNumber(root, "schema_version", byte.MaxValue);
        if (root.GetProperty("schema_version").GetByte() != 1)
        {
            throw new InvalidDataException("Unsupported support-bundle schema version.");
        }
        RequireUInt64(root, "policy_revision");
        RequireUInt64(root, "active_flows");
        RequireUInt64(root, "pending_probes");
        RequireUInt64(root, "sessions");
        RequireUInt64(root, "held_packets");
        RequireBoolean(root, "relay_configured");
        RequireUInt64(root, "relay_failover_count");
        RequireNullableUInt64(root, "relay_failover_convergence_ms");
        RequireUInt64(root, "uplink_failover_count");
        RequireNullableUInt64(root, "uplink_failover_convergence_ms");

        var counters = root.GetProperty("counters");
        RequireObject(counters, CounterFields, "support-bundle counters");
        foreach (var field in CounterFields)
        {
            RequireUInt64(counters, field);
        }

        var uplinks = root.GetProperty("uplinks");
        RequireArray(uplinks, MaxUplinks, "support-bundle uplinks");
        foreach (var uplink in uplinks.EnumerateArray())
        {
            RequireObject(uplink, UplinkFields, "support-bundle uplink");
            RequireNumber(uplink, "id", byte.MaxValue);
            RequireString(uplink, "state", 32);
            RequireUInt64(uplink, "rtt_ms");
            RequireNumber(uplink, "loss_permille", ushort.MaxValue);
            RequireUInt64(uplink, "capacity_bps");
            RequireNumber(uplink, "weight_permille", ushort.MaxValue);
            RequireBoolean(uplink, "metered");
            RequireBoolean(uplink, "eligible");
            RequireUInt64(uplink, "cwnd");
            RequireUInt64(uplink, "in_flight");
        }

        var rekey = root.GetProperty("rekey");
        RequireObject(rekey, RekeyFields, "support-bundle rekey data");
        RequireUInt64(rekey, "packets_since_rekey");
        RequireNullableUInt64(rekey, "last_rekey_ago_secs");
        RequireBoolean(rekey, "update_required");

        var congestion = root.GetProperty("congestion");
        RequireObject(congestion, CongestionFields, "support-bundle congestion data");
        RequireUInt64(congestion, "loss_events");
        RequireUInt64(congestion, "ecn_events");
        RequireUInt64(congestion, "delay_events");
        var paths = congestion.GetProperty("paths");
        RequireArray(paths, MaxUplinks, "support-bundle congestion paths");
        foreach (var path in paths.EnumerateArray())
        {
            RequireNumber(path, byte.MaxValue, "support-bundle congestion path id");
        }
        var perPath = congestion.GetProperty("per_path");
        RequireArray(perPath, MaxUplinks, "support-bundle per-path diagnostics");
        foreach (var path in perPath.EnumerateArray())
        {
            RequireObject(path, PathFields, "support-bundle path diagnostics");
            RequireNumber(path, "id", byte.MaxValue);
            RequireUInt64(path, "cwnd");
            RequireUInt64(path, "in_flight");
            RequireUInt64(path, "ecn_ce");
            RequireUInt64(path, "delay_events");
            RequireNullableUInt64(path, "rtt_min_ms");
            RequireNullableUInt64(path, "rtt_last_ms");
        }

        var warnings = root.GetProperty("warnings");
        RequireArray(warnings, MaxWarnings, "support-bundle warnings");
        foreach (var warning in warnings.EnumerateArray())
        {
            if (warning.ValueKind != JsonValueKind.String || warning.GetString() is not { Length: <= MaxWarningChars })
            {
                throw new InvalidDataException("Support-bundle warning is not a bounded string.");
            }
        }
    }

    private static void RequireObject(JsonElement element, IReadOnlyCollection<string> allowedFields, string label)
    {
        if (element.ValueKind != JsonValueKind.Object)
        {
            throw new InvalidDataException($"{label} must be a JSON object.");
        }

        var seen = new HashSet<string>(StringComparer.Ordinal);
        foreach (var property in element.EnumerateObject())
        {
            if (!seen.Add(property.Name) || !allowedFields.Contains(property.Name, StringComparer.Ordinal))
            {
                throw new InvalidDataException($"{label} contains a duplicate or unsupported field.");
            }
        }
        if (seen.Count != allowedFields.Count || allowedFields.Any(field => !seen.Contains(field)))
        {
            throw new InvalidDataException($"{label} is missing a required field.");
        }
    }

    private static void RequireArray(JsonElement element, int maxItems, string label)
    {
        if (element.ValueKind != JsonValueKind.Array || element.GetArrayLength() > maxItems)
        {
            throw new InvalidDataException($"{label} is invalid or exceeds its item limit.");
        }
    }

    private static void RequireString(JsonElement parent, string name, int maxChars)
    {
        var value = parent.GetProperty(name);
        if (value.ValueKind != JsonValueKind.String || value.GetString() is not { Length: > 0 } text || text.Length > maxChars)
        {
            throw new InvalidDataException($"Support-bundle field '{name}' is not a bounded string.");
        }
    }

    private static void RequireBoolean(JsonElement parent, string name)
    {
        if (parent.GetProperty(name).ValueKind is not (JsonValueKind.True or JsonValueKind.False))
        {
            throw new InvalidDataException($"Support-bundle field '{name}' is not a boolean.");
        }
    }

    private static void RequireUInt64(JsonElement parent, string name)
    {
        if (parent.GetProperty(name).ValueKind != JsonValueKind.Number || !parent.GetProperty(name).TryGetUInt64(out _))
        {
            throw new InvalidDataException($"Support-bundle field '{name}' is not an unsigned integer.");
        }
    }

    private static void RequireNullableUInt64(JsonElement parent, string name)
    {
        var value = parent.GetProperty(name);
        if (value.ValueKind != JsonValueKind.Null &&
            (value.ValueKind != JsonValueKind.Number || !value.TryGetUInt64(out _)))
        {
            throw new InvalidDataException($"Support-bundle field '{name}' is not an optional unsigned integer.");
        }
    }

    private static void RequireNumber(JsonElement parent, string name, ulong max)
    {
        var value = parent.GetProperty(name);
        if (value.ValueKind != JsonValueKind.Number || !value.TryGetUInt64(out var number) || number > max)
        {
            throw new InvalidDataException($"Support-bundle field '{name}' is outside its allowed numeric range.");
        }
    }

    private static void RequireNumber(JsonElement value, ulong max, string label)
    {
        if (value.ValueKind != JsonValueKind.Number || !value.TryGetUInt64(out var number) || number > max)
        {
            throw new InvalidDataException($"{label} is outside its allowed numeric range.");
        }
    }
}
