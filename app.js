const stationSelect = document.getElementById("stationSelect");
const departuresDiv = document.getElementById("departures");

let refreshIntervalId = null;

// Get a URL query parameter by name
function getQueryParam(param) {
  const urlParams = new URLSearchParams(window.location.search);
  return urlParams.get(param);
}

// Update a query parameter in the URL
function updateQueryParam(param, value) {
  const url = new URL(window.location);
  url.searchParams.set(param, value);
  window.history.replaceState({}, "", url);
}

// Fetch stations from TfL API
async function fetchStations() {
  const res = await fetch("https://api.tfl.gov.uk/StopPoint/Mode/tube");
  const data = await res.json();
  const stations = data.stopPoints
    .filter(sp => sp.commonName && sp.id.startsWith("940GZZLU"))
    .sort((a, b) => a.commonName.localeCompare(b.commonName));

  stations.forEach(station => {
    const opt = document.createElement("option");
    opt.value = station.id;
    opt.textContent = station.commonName;
    stationSelect.appendChild(opt);
  });

  // If a station is specified in the URL, select it
  const stationFromURL = getQueryParam("station");
  if (stationFromURL) {
    stationSelect.value = stationFromURL;
    stationSelect.dispatchEvent(new Event("change"));
  }
}

async function loadDepartures(stationId) {
  if (!stationId) {
    departuresDiv.innerHTML = "<p>Please select a station.</p>";
    return;
  }

  departuresDiv.innerHTML = "<p>Loading departures...</p>";

  try {
    const res = await fetch(`https://api.tfl.gov.uk/StopPoint/${stationId}/Arrivals`);
    const data = await res.json();

    const sorted = data.sort((a, b) => a.timeToStation - b.timeToStation);

    if (sorted.length === 0) {
      departuresDiv.innerHTML = "<p>No upcoming departures found.</p>";
      return;
    }

    departuresDiv.innerHTML = `
      <h2>Departures</h2>
      <table>
        <thead>
          <tr>
            <th>Line</th>
            <th>Destination</th>
            <th>Arrival (min)</th>
          </tr>
        </thead>
        <tbody>
          ${sorted.slice(0, 10).map(dep => `
            <tr>
              <td>${dep.lineName}</td>
              <td>${dep.destinationName}</td>
              <td>${Math.round(dep.timeToStation / 60)}</td>
            </tr>
          `).join("")}
        </tbody>
      </table>
    `;
  } catch (error) {
    departuresDiv.innerHTML = "<p>Error fetching departures. Please try again later.</p>";
    console.error(error);
  }
}

stationSelect.addEventListener("change", () => {
  const stationId = stationSelect.value;
  if (!stationId) {
    departuresDiv.innerHTML = "<p>Please select a station.</p>";
    if (refreshIntervalId) {
      clearInterval(refreshIntervalId);
      refreshIntervalId = null;
    }
    updateQueryParam("station", "");
    return;
  }

  // Update URL with selected station
  updateQueryParam("station", stationId);

  // Load departures initially
  loadDepartures(stationId);

  // Clear any existing interval
  if (refreshIntervalId) clearInterval(refreshIntervalId);

  // Set interval to refresh departures every 30 seconds
  refreshIntervalId = setInterval(() => {
    loadDepartures(stationId);
  }, 30000);
});

// Load stations on page load
fetchStations();

// Register Service Worker
if ("serviceWorker" in navigator) {
  navigator.serviceWorker.register("service-worker.js");
}
