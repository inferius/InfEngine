using System;

namespace InfEngine.Scripts.Host;

/// <summary>
/// Marks a static method as callable from the InfEngine blueprint
/// editor. The engine reflects over loaded assemblies at startup,
/// finds every method carrying this attribute, and registers a
/// blueprint node whose pins are derived from the method's
/// parameter and return types.
///
/// Supported parameter / return types (v1):
///   float, bool, (float,float,float) tuple (-&gt; Vec3 pin),
///   uint (-&gt; EntityRef pin), string.
///
/// Methods MUST be `public static`. Instance methods and generic
/// methods are rejected by the scanner with a clear error.
/// </summary>
[AttributeUsage(AttributeTargets.Method, AllowMultiple = false, Inherited = false)]
public sealed class BlueprintCallableAttribute : Attribute
{
    /// <summary>
    /// Optional display label shown in the blueprint context menu.
    /// Defaults to the method's name if omitted.
    /// </summary>
    public string? DisplayName { get; init; }

    /// <summary>
    /// Optional category for grouping in the context menu tree.
    /// Defaults to "Scripts" when omitted.
    /// </summary>
    public string? Category { get; init; }

    /// <summary>
    /// Optional tooltip shown on hover in the blueprint editor.
    /// </summary>
    public string? Tooltip { get; init; }
}
