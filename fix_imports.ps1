$path = "d:\Projects\CustomIDE\src\workflow.rs"
$content = Get-Content -Path $path -Raw
$old = "use std::sync::Arc;`r`nuse std::sync::atomic::{AtomicBool, AtomicU32, Ordering};`r`nuse std::sync::mpsc::{self, Receiver, Sender};"
$new = "use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};`r`nuse std::sync::mpsc::{Receiver, Sender};"
if ($content.Contains($old)) {
    $content = $content.Replace($old, $new)
    [System.IO.File]::WriteAllText($path, $content)
    Write-Output "replaced-crlf"
} else {
    $old2 = "use std::sync::Arc;`nuse std::sync::atomic::{AtomicBool, AtomicU32, Ordering};`nuse std::sync::mpsc::{self, Receiver, Sender};"
    $new2 = "use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};`nuse std::sync::mpsc::{Receiver, Sender};"
    if ($content.Contains($old2)) {
        $content = $content.Replace($old2, $new2)
        [System.IO.File]::WriteAllText($path, $content)
        Write-Output "replaced-lf"
    } else {
        Write-Output "not-found"
    }
}
