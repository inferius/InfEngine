# [BlueprintCallable] - C# methods exposed to blueprints

## Goal

A C# method tagged with `[BlueprintCallable]` should automatically
appear as a node in the blueprint editor's context menu, with its
parameters / return type wired to pins of the matching `PinType`.
Calling the node at run time invokes the C# method through the
existing CoreCLR hosting layer.

This mirrors Unreal's `UFUNCTION(BlueprintCallable)` pattern: the
C# author marks a method, the editor sees it, blueprints call it.

## Current state (as of commit 7a4f439)

What exists already:

- `infengine_scripting` hosts CoreCLR and loads a hot-reloadable
  game assembly (`scripts/InfEngine.Scripts.Game`).
- Reflection resolves a fixed `Player.OnTick(float dt)` entry
  point per tick.
- Communication goes C# -> Rust via `EngineApiPtrs` (static
  unmanaged callbacks), Rust -> C# is limited to the OnTick call.
- Blueprint VM has no mechanism to call into C# at all.

## Pipeline overview

Five pieces have to land:

1. **Attribute** (`InfEngine.Scripts.Host.BlueprintCallableAttribute`)
   - Declared in the host assembly so both library and game code can
     reference it.
   - Carries optional metadata: display name, category, tooltip.
2. **C# side metadata extraction**
   - On assembly load, walk every loaded assembly, find every
     `MethodInfo` with the attribute, extract its name + parameter
     types + return type, build a manifest.
3. **Rust ingestion**
   - Extend the FFI with a `register_blueprint_callable` callback so
     the C# host can stream the manifest into the Rust scripting
     crate at startup / reload.
   - Scripting crate stores the manifest in a global registry the
     blueprint crate can query.
4. **Editor integration**
   - At startup, blueprint editor calls into scripting to fetch the
     manifest and registers a dynamic `NodeKindDef` per method.
   - Pins typed by mapping C# types to `PinType` (float -> Float,
     bool -> Bool, `(float,float,float)` -> Vec3, `EntityRef` ->
     EntityRef, `string` -> Text).
5. **VM op + reverse call**
   - New `Op::CallCSharp { method: String, arg_count: u8 }` pops
     `arg_count` values, calls into the host, optionally pushes the
     return value.
   - `Host::call_csharp` (trait method) dispatches through the
     scripting crate's reverse-call surface (a `delegate*<>` cached
     per method from `MethodInfo.MethodHandle.GetFunctionPointer()`).

## Type marshaling

| C# type           | PinType    | Stack value     |
|-------------------|------------|-----------------|
| `float`           | Float      | `Value::Float`  |
| `bool`            | Bool       | `Value::Bool`   |
| `Vector3` (3-f32) | Vec3       | `Value::Vec3`   |
| `uint` (entity)   | EntityRef  | `Value::EntityRef` |
| `string`          | Text       | `Value::Text` *(new variant)* |

Reference types beyond `string` are out of scope for v1; the
attribute scanner rejects them with a clear error.

## What this commit ships

This is the foundation, not the full pipeline. Three pieces land
now so the rest can be filled in incrementally without a flag day:

1. **`BlueprintCallableAttribute`** in
   `scripts/InfEngine.Scripts.Host/BlueprintCallableAttribute.cs`.
   Marks methods; carries optional `Category` and `Tooltip`. Two
   existing host helpers (`Engine.Log`, `Engine.TimeSeconds`) get
   the attribute as examples.
2. **`call_cs_method`** blueprint node kind. `params["method"]`
   selects which method name to call. Inspector renders a text edit
   (a dropdown will replace it once the registry exists). Compiles
   to `Op::CallCSharp(method)`.
3. **Host trait surface**: `Host::call_csharp(method: &str)` with
   a default no-op impl. The editor's `PlayHost` logs a TODO line
   for now; the real CoreCLR reverse-call wiring will replace the
   stub in a follow-up.

## Remaining work (tracked here, not yet implemented)

- Reflection scan on assembly load, manifest streaming through FFI.
- Cached `delegate*<>` per attributed method.
- Dynamic `NodeKindDef` registration from the manifest.
- Argument marshaling at the call site (currently arg_count = 0).
- Return value handling.
- Editor-side dropdown populated from the registry.
- Validation that a referenced method still exists after hot
  reload; stale nodes get a "missing method" badge.
