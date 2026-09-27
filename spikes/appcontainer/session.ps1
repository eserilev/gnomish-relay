param([switch]$Create)
"session: " + (Get-Process -Id $PID).SessionId
Add-Type @"
using System; using System.Runtime.InteropServices;
public static class N {
  [StructLayout(LayoutKind.Sequential)] public struct US { public ushort L; public ushort M; public IntPtr B; }
  [StructLayout(LayoutKind.Sequential)] public struct OA { public int Len; public IntPtr Root; public IntPtr Name; public uint Attr; public IntPtr SD; public IntPtr SQ; }
  [DllImport("ntdll.dll")] public static extern int NtOpenDirectoryObject(out IntPtr h, uint access, ref OA oa);
  [DllImport("ntdll.dll")] public static extern int NtCreateDirectoryObject(out IntPtr h, uint access, ref OA oa);
  static OA Oa(string path) {
    var s = Marshal.StringToHGlobalUni(path);
    var us = new US { L = (ushort)(path.Length * 2), M = (ushort)(path.Length * 2 + 2), B = s };
    var p = Marshal.AllocHGlobal(Marshal.SizeOf(typeof(US)));
    Marshal.StructureToPtr(us, p, false);
    return new OA { Len = Marshal.SizeOf(typeof(OA)), Name = p, Attr = 0x40 };
  }
  public static string Open(string path) { var oa = Oa(path); IntPtr h; return NtOpenDirectoryObject(out h, 1, ref oa).ToString("X"); }
  public static string Create(string path) { var oa = Oa(path); IntPtr h; return NtCreateDirectoryObject(out h, 0xF000F, ref oa).ToString("X"); }
}
"@
$sid = (Get-Process -Id $PID).SessionId
foreach ($p in @("\Sessions\$sid\AppContainerNamedObjects", "\Sessions\$sid\BaseNamedObjects", "\Sessions\1\AppContainerNamedObjects", "\Sessions\0\AppContainerNamedObjects", "\BaseNamedObjects")) {
  "open $p -> " + [N]::Open($p)
}
if ($Create) {
  "create -> " + [N]::Create("\Sessions\$sid\AppContainerNamedObjects")
  "open again -> " + [N]::Open("\Sessions\$sid\AppContainerNamedObjects")
}
