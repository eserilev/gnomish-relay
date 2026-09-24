import Protocol.Spec.Bytes

/-!
# What a frame is

```text
[0x6E 0x52] [version] [time: 4] [frame id: 2] [len: 2] [payload] [fletcher16: 2] [tag: 8]
```
-/

namespace Protocol.Spec

def maxPayload : Nat := 3200

/-- Everything that the checksum covers: version to payload. -/
def frameChecked (time frameId : Nat) (payload : List Byte) : List Byte :=
  [1] ++ be32 time ++ be16 frameId ++ be16 payload.length ++ payload

/-- Everything that the tag covers: magic to checksum. -/
def frameSigned (time frameId : Nat) (payload : List Byte) : List Byte :=
  let checked := frameChecked time frameId payload
  [0x6E, 0x52] ++ checked ++ fletcher16 checked

def frameBytes (time frameId : Nat) (payload tag : List Byte) : List Byte :=
  frameSigned time frameId payload ++ tag

/-- The bridge accepts a frame at most 5 minutes old and at most 1 minute ahead. -/
def fresh (frameTime now : Nat) : Prop :=
  now ≤ frameTime + 300 ∧ frameTime ≤ now + 60

end Protocol.Spec
