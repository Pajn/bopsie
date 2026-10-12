package expo.modules.cratieplayer

import kotlin.math.abs
import kotlin.math.roundToLong

/**
 * Sendspin's two-dimensional Kalman filter over the clock offset and drift
 * between this device and the server, fed by NTP-style `client/time`
 * exchanges. A port of aiosendspin's `SendspinTimeFilter` (itself a port of
 * the ESPHome implementation); see https://github.com/Sendspin/time-filter.
 *
 * All times are microseconds. Client times must come from a monotonic clock.
 * Not thread-safe: the owner serializes updates and conversions.
 */
class TimeFilter(
  processStdDev: Double = 0.0,
  forgetFactor: Double = 2.0,
  driftProcessStdDev: Double = 1e-11,
) {
  private val processVariance = processStdDev * processStdDev
  private val driftProcessVariance = driftProcessStdDev * driftProcessStdDev
  private val forgetVarianceFactor = forgetFactor * forgetFactor

  private var lastUpdate = 0L
  var count = 0
    private set
  private var offset = 0.0
  private var drift = 0.0
  private var offsetCovariance = Double.POSITIVE_INFINITY
  private var offsetDriftCovariance = 0.0
  private var driftCovariance = 0.0

  /** The transform conversions use, published after each update. */
  private var element = Element(0L, 0.0, 0.0, false)

  private data class Element(val lastUpdate: Long, val offset: Double, val drift: Double, val useDrift: Boolean)

  /** Ready once two measurements give a finite offset uncertainty. */
  val isSynchronized: Boolean get() = count >= 2 && offsetCovariance.isFinite()

  /** Standard deviation of the offset estimate, in microseconds. */
  val errorUs: Long get() = if (offsetCovariance.isFinite()) Math.sqrt(offsetCovariance).roundToLong() else Long.MAX_VALUE

  /**
   * @param measurement `((T2 - T1) + (T3 - T4)) / 2`
   * @param maxError `((T4 - T1) - (T3 - T2)) / 2`, half the round trip spent on the network
   * @param timeAdded the client time `T4` at which the reply arrived
   */
  fun update(measurement: Long, maxError: Long, timeAdded: Long) {
    if (timeAdded <= lastUpdate) return
    val dt = (timeAdded - lastUpdate).toDouble()
    lastUpdate = timeAdded

    val updateStdDev = maxError.toDouble() * MAX_ERROR_SCALE
    val measurementVariance = updateStdDev * updateStdDev

    if (count <= 0) {
      count++
      offset = measurement.toDouble()
      offsetCovariance = measurementVariance
      drift = 0.0
      element = Element(lastUpdate, offset, drift, false)
      return
    }

    if (count == 1) {
      count++
      drift = (measurement - offset) / dt
      offset = measurement.toDouble()
      driftCovariance = (offsetCovariance + measurementVariance) / (dt * dt)
      offsetCovariance = measurementVariance
      element = Element(lastUpdate, offset, drift, false)
      return
    }

    // Predict.
    val predictedOffset = offset + drift * dt
    var newDriftCovariance = driftCovariance + dt * driftProcessVariance
    var newOffsetDriftCovariance = offsetDriftCovariance + driftCovariance * dt
    var newOffsetCovariance = offsetCovariance + 2 * offsetDriftCovariance * dt +
      driftCovariance * dt * dt + dt * processVariance

    // Adaptive forgetting: a residual far beyond the measurement's own error
    // means the network or the server clock jumped; trust new data more.
    val residual = measurement - predictedOffset
    if (count < MIN_SAMPLES_FOR_FORGETTING) {
      count++
    } else if (abs(residual) > maxError * ADAPTIVE_FORGETTING_CUTOFF) {
      newDriftCovariance *= forgetVarianceFactor
      newOffsetDriftCovariance *= forgetVarianceFactor
      newOffsetCovariance *= forgetVarianceFactor
    }

    // Update.
    val uncertainty = 1.0 / maxOf(newOffsetCovariance + measurementVariance, 1e-9)
    val offsetGain = newOffsetCovariance * uncertainty
    val driftGain = newOffsetDriftCovariance * uncertainty
    offset = predictedOffset + offsetGain * residual
    drift += driftGain * residual
    driftCovariance = newDriftCovariance - driftGain * newOffsetDriftCovariance
    offsetDriftCovariance = newOffsetDriftCovariance - driftGain * newOffsetCovariance
    offsetCovariance = newOffsetCovariance - offsetGain * newOffsetCovariance

    val useDrift = drift * drift > DRIFT_SIGNIFICANCE_THRESHOLD_SQUARED * driftCovariance
    element = Element(lastUpdate, offset, drift, useDrift)
  }

  fun serverTime(clientTime: Long): Long {
    val e = element
    val effectiveDrift = if (e.useDrift) e.drift else 0.0
    return clientTime + (e.offset + effectiveDrift * (clientTime - e.lastUpdate)).roundToLong()
  }

  fun clientTime(serverTime: Long): Long {
    val e = element
    val effectiveDrift = if (e.useDrift) e.drift else 0.0
    return ((serverTime - e.offset + effectiveDrift * e.lastUpdate) / (1.0 + effectiveDrift)).roundToLong()
  }

  fun reset() {
    count = 0
    lastUpdate = 0
    offset = 0.0
    drift = 0.0
    offsetCovariance = Double.POSITIVE_INFINITY
    offsetDriftCovariance = 0.0
    driftCovariance = 0.0
    element = Element(0L, 0.0, 0.0, false)
  }

  companion object {
    private const val ADAPTIVE_FORGETTING_CUTOFF = 3.0
    private const val MAX_ERROR_SCALE = 0.5
    private const val DRIFT_SIGNIFICANCE_THRESHOLD_SQUARED = 2.0 * 2.0
    private const val MIN_SAMPLES_FOR_FORGETTING = 100
  }
}
