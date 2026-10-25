using System.Buffers.Binary;
using System.IO.Pipes;
using System.Security.Principal;
using System.Text;

namespace BalanciFy.App;

internal static class IpcClient
{
    private const string PipeName = "BalanciFy.Service.v1";
    private const int MaxPayload = 1 << 20;
    private const byte SchemaVersion = 1;
    private const byte RequestKind = 1;
    private const byte ResponseKind = 2;
    private const byte GetStatusOpcode = 1;
    private const byte ListFlowRulesOpcode = 3;
    private const byte RunSelfTestOpcode = 9;
    private const byte GetSupportBundleOpcode = 11;
    private const byte SupportBundleResponseTag = 8;
    private const byte SetFlowRuleOpcode = 4;
    private const byte RemoveFlowRuleOpcode = 5;
    private const byte SetBondModeOpcode = 6;
    private const byte SetDuplicateModeOpcode = 7;
    private const byte SetAllowMeteredOpcode = 8;
    private const byte AckResponseTag = 5;
    private const byte StatusResponseTag = 2;
    private const byte FlowRulesResponseTag = 4;
    private const byte SelfTestResponseTag = 6;
    private static readonly UTF8Encoding StrictUtf8 = new(false, true);

    public static async Task<ServiceSnapshot> GetStatusAsync(CancellationToken cancellationToken) =>
        DecodeStatus(await SendRequestAsync(GetStatusOpcode, cancellationToken).ConfigureAwait(false));

    public static async Task<IReadOnlyList<FlowRuleSnapshot>> GetFlowRulesAsync(CancellationToken cancellationToken)
    {
        var reader = new PayloadReader(await SendRequestAsync(ListFlowRulesOpcode, cancellationToken).ConfigureAwait(false));
        var responseTag = reader.ReadByte();
        ThrowServiceError(ref reader, responseTag);
        if (responseTag != FlowRulesResponseTag)
        {
            throw new InvalidDataException($"Expected a flow-rules response, received tag {responseTag}.");
        }
        var count = reader.ReadUInt32();
        if (count > 256)
        {
            throw new InvalidDataException("Service returned too many flow rules.");
        }
        var rules = new List<FlowRuleSnapshot>((int)count);
        for (var i = 0; i < count; i++)
        {
            rules.Add(new FlowRuleSnapshot(
                reader.ReadUInt32(),
                reader.ReadString8(),
                reader.ReadByte(),
                reader.ReadByte(),
                reader.ReadUInt32(),
                reader.ReadByte(),
                reader.ReadUInt16(),
                reader.ReadUInt16(),
                reader.ReadBoolean()));
        }
        reader.RequireEnd();
        return rules;
    }

    public static async Task<SelfTestSnapshot> RunSelfTestAsync(CancellationToken cancellationToken)
    {
        var reader = new PayloadReader(await SendRequestAsync(RunSelfTestOpcode, cancellationToken).ConfigureAwait(false));
        var responseTag = reader.ReadByte();
        ThrowServiceError(ref reader, responseTag);
        if (responseTag != SelfTestResponseTag)
        {
            throw new InvalidDataException($"Expected a self-test response, received tag {responseTag}.");
        }
        var result = new SelfTestSnapshot(reader.ReadBoolean(), reader.ReadString8());
        reader.RequireEnd();
        return result;
    }

    public static async Task<string> GetSupportBundleAsync(CancellationToken cancellationToken)
    {
        var reader = new PayloadReader(await SendRequestAsync(GetSupportBundleOpcode, cancellationToken).ConfigureAwait(false));
        var responseTag = reader.ReadByte();
        ThrowServiceError(ref reader, responseTag);
        if (responseTag != SupportBundleResponseTag)
        {
            throw new InvalidDataException($"Expected a support-bundle response, received tag {responseTag}.");
        }
        var bundle = reader.ReadString32();
        reader.RequireEnd();
        return bundle;
    }

    public static Task SetBondModeAsync(bool enabled, CancellationToken cancellationToken) =>
        SendAckRequestAsync(new[] { SetBondModeOpcode, enabled ? (byte)1 : (byte)0 }, cancellationToken);

    public static Task SetDuplicateModeAsync(bool enabled, CancellationToken cancellationToken) =>
        SendAckRequestAsync(new[] { SetDuplicateModeOpcode, enabled ? (byte)1 : (byte)0 }, cancellationToken);

    public static Task SetAllowMeteredAsync(bool allowed, CancellationToken cancellationToken) =>
        SendAckRequestAsync(new[] { SetAllowMeteredOpcode, allowed ? (byte)1 : (byte)0 }, cancellationToken);

    public static Task RemoveFlowRuleAsync(uint id, CancellationToken cancellationToken)
    {
        var payload = new byte[5];
        payload[0] = RemoveFlowRuleOpcode;
        BinaryPrimitives.WriteUInt32BigEndian(payload.AsSpan(1), id);
        return SendAckRequestAsync(payload, cancellationToken);
    }

    public static Task SetFlowRuleAsync(FlowRuleSnapshot rule, CancellationToken cancellationToken)
    {
        var name = StrictUtf8.GetBytes(rule.Name);
        if (name.Length is 0 or > byte.MaxValue)
        {
            throw new ArgumentException("Rule names must contain 1 to 255 UTF-8 bytes.", nameof(rule));
        }

        var payload = new List<byte>(18 + name.Length) { SetFlowRuleOpcode };
        AppendUInt32(payload, rule.Id);
        payload.Add((byte)name.Length);
        payload.AddRange(name);
        payload.Add(rule.Class);
        payload.Add(rule.PinnedUplink);
        AppendUInt32(payload, rule.Priority);
        payload.Add(rule.Protocol);
        AppendUInt16(payload, rule.PortStart);
        AppendUInt16(payload, rule.PortEnd);
        payload.Add(rule.DuplicateEligible ? (byte)1 : (byte)0);
        return SendAckRequestAsync(payload.ToArray(), cancellationToken);
    }

    private static async Task SendAckRequestAsync(byte[] requestPayload, CancellationToken cancellationToken)
    {
        var response = await SendRequestAsync(requestPayload, cancellationToken).ConfigureAwait(false);
        var reader = new PayloadReader(response);
        var responseTag = reader.ReadByte();
        ThrowServiceError(ref reader, responseTag);
        if (responseTag != AckResponseTag)
        {
            throw new InvalidDataException($"Expected an acknowledgement, received tag {responseTag}.");
        }
        reader.RequireEnd();
    }

    private static void AppendUInt16(List<byte> buffer, ushort value)
    {
        Span<byte> encoded = stackalloc byte[sizeof(ushort)];
        BinaryPrimitives.WriteUInt16BigEndian(encoded, value);
        buffer.AddRange(encoded.ToArray());
    }

    private static void AppendUInt32(List<byte> buffer, uint value)
    {
        Span<byte> encoded = stackalloc byte[sizeof(uint)];
        BinaryPrimitives.WriteUInt32BigEndian(encoded, value);
        buffer.AddRange(encoded.ToArray());
    }

    private static Task<byte[]> SendRequestAsync(byte opcode, CancellationToken cancellationToken) =>
        SendRequestAsync(new[] { opcode }, cancellationToken);

    private static async Task<byte[]> SendRequestAsync(byte[] requestPayload, CancellationToken cancellationToken)
    {
        if (requestPayload.Length == 0 || requestPayload.Length > MaxPayload)
        {
            throw new ArgumentOutOfRangeException(nameof(requestPayload));
        }
        await using var pipe = new NamedPipeClientStream(
            ".",
            PipeName,
            PipeDirection.InOut,
            PipeOptions.Asynchronous,
            TokenImpersonationLevel.Impersonation);
        await pipe.ConnectAsync(5_000, cancellationToken).ConfigureAwait(false);

        // IPC v1: version byte, kind byte, big-endian payload length, request payload.
        var request = new byte[6 + requestPayload.Length];
        request[0] = SchemaVersion;
        request[1] = RequestKind;
        BinaryPrimitives.WriteUInt32BigEndian(request.AsSpan(2, 4), (uint)requestPayload.Length);
        requestPayload.CopyTo(request, 6);
        await pipe.WriteAsync(request, cancellationToken).ConfigureAwait(false);
        await pipe.FlushAsync(cancellationToken).ConfigureAwait(false);

        var header = await ReadExactlyAsync(pipe, 6, cancellationToken).ConfigureAwait(false);
        if (header[0] != SchemaVersion || header[1] != ResponseKind)
        {
            throw new InvalidDataException("Service returned an unsupported IPC frame header.");
        }
        var payloadLength = BinaryPrimitives.ReadUInt32BigEndian(header.AsSpan(2, 4));
        if (payloadLength == 0 || payloadLength > (uint)MaxPayload)
        {
            throw new InvalidDataException("Service returned an invalid IPC payload length.");
        }
        return await ReadExactlyAsync(pipe, checked((int)payloadLength), cancellationToken).ConfigureAwait(false);
    }

    private static async Task<byte[]> ReadExactlyAsync(
        Stream stream,
        int length,
        CancellationToken cancellationToken)
    {
        var buffer = new byte[length];
        var offset = 0;
        while (offset < buffer.Length)
        {
            var read = await stream.ReadAsync(buffer.AsMemory(offset), cancellationToken).ConfigureAwait(false);
            if (read == 0)
            {
                throw new EndOfStreamException("Service closed the IPC stream mid-frame.");
            }
            offset += read;
        }
        return buffer;
    }

    private static void ThrowServiceError(ref PayloadReader reader, byte responseTag)
    {
        if (responseTag != 7)
        {
            return;
        }
        var status = reader.ReadByte();
        var message = reader.ReadString8();
        reader.RequireEnd();
        throw new IOException($"Service rejected the request (code {status}): {message}");
    }

    private static ServiceSnapshot DecodeStatus(ReadOnlySpan<byte> payload)
    {
        var reader = new PayloadReader(payload);
        var responseTag = reader.ReadByte();
        ThrowServiceError(ref reader, responseTag);
        if (responseTag != StatusResponseTag)
        {
            throw new InvalidDataException($"Expected a status response, received tag {responseTag}.");
        }

        var serviceVersion = reader.ReadString8();
        var schemaVersion = reader.ReadByte();
        var policyRevision = reader.ReadUInt64();
        _ = reader.ReadUInt64(); // rules revision
        var bondEnabled = reader.ReadBoolean();
        var duplicateMode = reader.ReadBoolean();
        var allowMetered = reader.ReadBoolean();
        var adapterMtu = reader.ReadUInt16();
        var uplinkCount = reader.ReadUInt32();
        if (uplinkCount > 256)
        {
            throw new InvalidDataException("Service returned too many uplinks.");
        }
        var uplinks = new List<UplinkSnapshot>((int)uplinkCount);
        for (var i = 0; i < uplinkCount; i++)
        {
            uplinks.Add(new UplinkSnapshot(
                reader.ReadByte(),
                reader.ReadByte(),
                reader.ReadUInt32(),
                reader.ReadUInt16(),
                reader.ReadUInt64(),
                reader.ReadUInt16(),
                reader.ReadBoolean(),
                reader.ReadBoolean()));
        }

        var activeFlows = reader.ReadUInt32();
        var packetsStripped = reader.ReadUInt64();
        var packetsForwarded = reader.ReadUInt64();
        _ = reader.ReadUInt64(); // duplicates sent
        _ = reader.ReadUInt64(); // reinjections
        _ = reader.ReadUInt64(); // delivered
        _ = reader.ReadUInt64(); // out of order
        var refused = reader.ReadUInt64();
        var authenticationFailures = reader.ReadUInt64();
        var ecnCe = reader.Remaining >= sizeof(ulong) ? reader.ReadUInt64() : 0;
        var delayEvents = reader.Remaining >= sizeof(ulong) ? reader.ReadUInt64() : 0;
        reader.RequireEnd();

        return new ServiceSnapshot(
            serviceVersion,
            schemaVersion,
            policyRevision,
            bondEnabled,
            duplicateMode,
            allowMetered,
            adapterMtu,
            activeFlows,
            packetsStripped,
            packetsForwarded,
            refused,
            authenticationFailures,
            ecnCe,
            delayEvents,
            uplinks);
    }

    private ref struct PayloadReader
    {
        private readonly ReadOnlySpan<byte> _payload;
        private int _offset;

        public PayloadReader(ReadOnlySpan<byte> payload)
        {
            _payload = payload;
            _offset = 0;
        }

        public byte ReadByte()
        {
            EnsureAvailable(1);
            return _payload[_offset++];
        }

        public bool ReadBoolean() => ReadByte() switch
        {
            0 => false,
            1 => true,
            _ => throw new InvalidDataException("IPC boolean field must be zero or one."),
        };

        public ushort ReadUInt16()
        {
            EnsureAvailable(sizeof(ushort));
            var value = BinaryPrimitives.ReadUInt16BigEndian(_payload.Slice(_offset, sizeof(ushort)));
            _offset += sizeof(ushort);
            return value;
        }

        public uint ReadUInt32()
        {
            EnsureAvailable(sizeof(uint));
            var value = BinaryPrimitives.ReadUInt32BigEndian(_payload.Slice(_offset, sizeof(uint)));
            _offset += sizeof(uint);
            return value;
        }

        public ulong ReadUInt64()
        {
            EnsureAvailable(sizeof(ulong));
            var value = BinaryPrimitives.ReadUInt64BigEndian(_payload.Slice(_offset, sizeof(ulong)));
            _offset += sizeof(ulong);
            return value;
        }

        public string ReadString8() => ReadUtf8(ReadByte());

        public string ReadString32()
        {
            var length = ReadUInt32();
            if (length > (uint)MaxPayload)
            {
                throw new InvalidDataException("IPC string exceeds the payload limit.");
            }
            return ReadUtf8((int)length);
        }

        public int Remaining => _payload.Length - _offset;

        private string ReadUtf8(int length)
        {
            EnsureAvailable(length);
            string result;
            try
            {
                result = StrictUtf8.GetString(_payload.Slice(_offset, length));
            }
            catch (DecoderFallbackException error)
            {
                throw new InvalidDataException("IPC string is not valid UTF-8.", error);
            }
            _offset += length;
            return result;
        }

        public void RequireEnd()
        {
            if (_offset != _payload.Length)
            {
                throw new InvalidDataException("IPC response contains trailing bytes.");
            }
        }

        private void EnsureAvailable(int length)
        {
            if (length < 0 || _offset > _payload.Length - length)
            {
                throw new InvalidDataException("IPC response is truncated.");
            }
        }
    }
}

internal sealed record ServiceSnapshot(
    string ServiceVersion,
    byte SchemaVersion,
    ulong PolicyRevision,
    bool BondEnabled,
    bool DuplicateMode,
    bool AllowMetered,
    ushort AdapterMtu,
    uint ActiveFlows,
    ulong PacketsStripped,
    ulong PacketsForwarded,
    ulong Refused,
    ulong AuthenticationFailures,
    ulong EcnCe,
    ulong DelayEvents,
    IReadOnlyList<UplinkSnapshot> Uplinks);

internal sealed record UplinkSnapshot(
    byte Id,
    byte State,
    uint RttMs,
    ushort LossPermille,
    ulong CapacityBps,
    ushort WeightPermille,
    bool Metered,
    bool Eligible);

internal sealed record FlowRuleSnapshot(
    uint Id,
    string Name,
    byte Class,
    byte PinnedUplink,
    uint Priority,
    byte Protocol,
    ushort PortStart,
    ushort PortEnd,
    bool DuplicateEligible);

internal sealed record SelfTestSnapshot(bool Passed, string Detail);
