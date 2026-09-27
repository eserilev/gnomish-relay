Add-Type @"
using System; using System.Runtime.InteropServices;
public static class AC {
  [StructLayout(LayoutKind.Sequential)] public struct SC { public IntPtr Sid; public IntPtr Caps; public uint Count; public uint Reserved; }
  [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)] public struct SI {
    public int cb; public string r; public string d; public string t; public int x; public int y; public int xs; public int ys; public int xc; public int yc; public int fa; public int fl; public short sw; public short r2; public IntPtr r3; public IntPtr i; public IntPtr o; public IntPtr e; }
  [StructLayout(LayoutKind.Sequential)] public struct SIX { public SI si; public IntPtr attrs; }
  [StructLayout(LayoutKind.Sequential)] public struct PI { public IntPtr hp; public IntPtr ht; public int pid; public int tid; }
  [DllImport("userenv.dll", CharSet = CharSet.Unicode)] public static extern int CreateAppContainerProfile(string n, string d, string desc, IntPtr caps, uint count, out IntPtr sid);
  [DllImport("userenv.dll", CharSet = CharSet.Unicode)] public static extern int DeriveAppContainerSidFromAppContainerName(string n, out IntPtr sid);
  [DllImport("userenv.dll", CharSet = CharSet.Unicode)] public static extern int DeleteAppContainerProfile(string n);
  [DllImport("kernel32.dll", SetLastError = true)] public static extern bool InitializeProcThreadAttributeList(IntPtr l, int c, int f, ref IntPtr size);
  [DllImport("kernel32.dll", SetLastError = true)] public static extern bool UpdateProcThreadAttribute(IntPtr l, uint f, IntPtr a, IntPtr v, IntPtr s, IntPtr p, IntPtr r);
  [DllImport("kernel32.dll", SetLastError = true, CharSet = CharSet.Unicode)] public static extern bool CreateProcessW(string app, System.Text.StringBuilder cmd, IntPtr pa, IntPtr ta, bool inh, uint flags, IntPtr env, string cwd, ref SIX si, out PI pi);
  [DllImport("kernel32.dll")] public static extern uint WaitForSingleObject(IntPtr h, uint ms);
  [DllImport("kernel32.dll")] public static extern bool GetExitCodeProcess(IntPtr h, out uint c);
  public static string Run(string name, bool profile, string cmd) {
    IntPtr sid; int hr;
    if (profile) {
      hr = CreateAppContainerProfile(name, name, name, IntPtr.Zero, 0, out sid);
      if (hr != 0) { string r = "create hr=" + hr.ToString("X"); hr = DeriveAppContainerSidFromAppContainerName(name, out sid); r += " derive hr=" + hr.ToString("X"); Console.WriteLine(r); }
    } else { hr = DeriveAppContainerSidFromAppContainerName(name, out sid); }
    var sc = new SC { Sid = sid, Caps = IntPtr.Zero, Count = 0, Reserved = 0 };
    IntPtr scp = Marshal.AllocHGlobal(Marshal.SizeOf(sc)); Marshal.StructureToPtr(sc, scp, false);
    IntPtr size = IntPtr.Zero; InitializeProcThreadAttributeList(IntPtr.Zero, 1, 0, ref size);
    IntPtr list = Marshal.AllocHGlobal(size);
    if (!InitializeProcThreadAttributeList(list, 1, 0, ref size)) return "init " + Marshal.GetLastWin32Error();
    if (!UpdateProcThreadAttribute(list, 0, (IntPtr)0x20009, scp, (IntPtr)Marshal.SizeOf(sc), IntPtr.Zero, IntPtr.Zero)) return "update " + Marshal.GetLastWin32Error();
    var six = new SIX(); six.si.cb = Marshal.SizeOf(six); six.attrs = list;
    PI pi;
    bool ok = CreateProcessW("C:\\Windows\\System32\\cmd.exe", new System.Text.StringBuilder(cmd), IntPtr.Zero, IntPtr.Zero, false, 0x00080000, IntPtr.Zero, "C:\\Windows\\System32", ref six, out pi);
    if (!ok) return "createprocess err=" + Marshal.GetLastWin32Error();
    WaitForSingleObject(pi.hp, 10000); uint code; GetExitCodeProcess(pi.hp, out code);
    if (profile) DeleteAppContainerProfile(name);
    return "ok exit=" + code;
  }
}
"@
New-Item -ItemType Directory -Force C:\actest | Out-Null
icacls C:\actest /grant "*S-1-15-2-1:(OI)(CI)M" | Out-Null
"no profile: " + [AC]::Run("gnomish.plain.a", $false, "cmd /c echo hi> C:\actest\a.txt")
"profile: " + [AC]::Run("gnomish.plain.b", $true, "cmd /c echo hi> C:\actest\b.txt")
Get-ChildItem C:\actest
