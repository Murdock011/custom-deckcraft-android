package ai.storyteller.deckcraft.nativeapp

import android.app.Activity
import android.content.Intent
import android.os.Bundle
import android.os.Process

/**
 * Restarts the app. It runs in its own process (`:restart`, see the manifest) so it survives the kill of the
 * main one. [MainActivity] starts it when the system recreates the activity in a process whose native UI loop
 * has already run (the loop can run only once per process). It kills the old process, starts a fresh
 * MainActivity in a new one (which opens on Recover when there was unsaved work), and ends itself.
 */
class RestartActivity : Activity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val pid = intent.getIntExtra("pid", -1)
        if (pid > 0 && pid != Process.myPid()) Process.killProcess(pid)
        startActivity(Intent(this, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TASK))
        finish()
        // This helper process has nothing left to do; end it so it does not linger.
        Runtime.getRuntime().exit(0)
    }
}
