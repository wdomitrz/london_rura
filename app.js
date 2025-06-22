const stationSelect = document.getElementById("stationSelect");
const departuresDiv = document.getElementById("departures");

let refreshIntervalId = null;

function getQueryParam(param) {
  const urlParams = new URLSearchParams(window.location.search);
  return urlParams.get(param);
}

function updateQueryParam(param, value) {
  const url = new URL(window.location);
  url.searchParams.set(param, value);
  window.history.replaceState({}, "", url);
}

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

    const sorted = data
      .filter(dep => dep.destinationName)
      .sort((a, b) => a.timeToStation - b.timeToStation);

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

  updateQueryParam("station", stationId);
  loadDepartures(stationId);
  if (refreshIntervalId) clearInterval(refreshIntervalId);
  refreshIntervalId = setInterval(() => {
    loadDepartures(stationId);
  }, 30000);
});

fetchStations();

if ("serviceWorker" in navigator) {
  navigator.serviceWorker.register("service-worker.js");
}
