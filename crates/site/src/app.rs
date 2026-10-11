use leptos::prelude::*;

use crate::components::{
    DogfoodBanner, Example, Features, Footer, Hero, HowItWorks, Install, Roadmap, VersionSwitcher,
};

/// Root of the landing page. Each section is a component so the view code
/// carries over unchanged if the build ever moves from CSR/Trunk to SSR.
#[component]
pub fn App() -> impl IntoView {
    view! {
        <div class="min-h-screen text-chalk-50 antialiased">
            <VersionSwitcher/>
            <Hero/>
            <Install/>
            <HowItWorks/>
            <Example/>
            <Features/>
            <DogfoodBanner/>
            <Roadmap/>
            <Footer/>
        </div>
    }
}
