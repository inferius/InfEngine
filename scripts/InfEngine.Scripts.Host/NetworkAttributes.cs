using System;

namespace InfEngine.Scripts.Host;

/// <summary>
/// Marks a static method that runs only on the networking server.
/// Clients calling it route the invocation through the server via
/// <see cref="Engine.NetSendEvent"/>; the server executes the body
/// locally. v1 marshalling is the same single-float payload the BP
/// `Send Net Event` node uses; richer marshalling is roadmap.
///
/// Methods MUST be <c>public static void Method(float)</c> for the
/// dispatcher to wire them. Mismatched signatures are rejected at
/// scan time with a log line.
/// </summary>
[AttributeUsage(AttributeTargets.Method, AllowMultiple = false, Inherited = false)]
public sealed class ServerRpcAttribute : Attribute
{
    /// <summary>Network event name. Defaults to the method name.</summary>
    public string? EventName { get; init; }
}

/// <summary>
/// Marks a static method that runs only on connected clients. The
/// server emits the event via <see cref="Engine.NetSendEvent"/>; each
/// client executes the method body locally when the event arrives.
/// Same signature contract as <see cref="ServerRpcAttribute"/>.
/// </summary>
[AttributeUsage(AttributeTargets.Method, AllowMultiple = false, Inherited = false)]
public sealed class ClientRpcAttribute : Attribute
{
    /// <summary>Network event name. Defaults to the method name.</summary>
    public string? EventName { get; init; }
}
