package com.ch3n4y.easytier_manager

import android.app.Activity
import android.content.Intent
import androidx.core.content.FileProvider
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.Plugin
import java.io.File

@InvokeArg
class InstallApkArgs {
    var path: String? = null
}

/**
 * Hands a downloaded APK to Android's package installer.
 *
 * An app cannot install a package itself — the platform requires the user to
 * confirm in a dialog it owns. All this does is build that intent. The file has
 * to sit under a path exposed by `res/xml/file_paths.xml`, which is why the
 * download target is the app cache directory, and the authority has to match
 * the provider declared in the manifest.
 */
@TauriPlugin
class ApkInstallerPlugin(private val activity: Activity) : Plugin(activity) {
    @Command
    fun install(invoke: Invoke) {
        val path = invoke.parseArgs(InstallApkArgs::class.java).path
        if (path.isNullOrBlank()) {
            invoke.reject("缺少安装包路径")
            return
        }

        val apk = File(path)
        if (!apk.isFile) {
            invoke.reject("安装包不存在，请重新下载")
            return
        }

        try {
            val uri = FileProvider.getUriForFile(
                activity,
                "${activity.packageName}.fileprovider",
                apk,
            )
            val intent = Intent(Intent.ACTION_VIEW).apply {
                setDataAndType(uri, "application/vnd.android.package-archive")
                addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
                addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
            }
            activity.startActivity(intent)
            invoke.resolve()
        } catch (error: Exception) {
            invoke.reject(error.message ?: "无法启动系统安装器")
        }
    }
}
