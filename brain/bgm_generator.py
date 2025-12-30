import os
from google import genai
from google.genai import types


def generate_bgm_with_gemini(mood: str, duration: int, output_path: str) -> str:
    """
    Generate background music using Gemini 2.0 Flash
    
    Args:
        mood: "energetic", "calm", "mysterious", "upbeat"
        duration: Target duration in seconds (30-60s recommended for loops)
        output_path: Where to save generated audio
    
    Returns:
        Path to generated BGM file, or None if generation failed
    """
    try:
        client = genai.Client(api_key=os.environ["GEMINI_API_KEY"])
        
        prompt = f"""Generate a {duration}-second {mood} background music track 
suitable for YouTube videos. 
Style: modern, copyright-free, loopable.
Instruments: {"upbeat synth, drums" if mood == "energetic" else "soft piano, ambient pads"}
No vocals."""
        
        # Use Gemini 2.0 Flash audio generation (experimental feature)
        # Note: This feature may not be available in all regions yet
        response = client.models.generate_content(
            model="gemini-2.5-flash",
            contents=[prompt],
            config=types.GenerateContentConfig(
                response_modalities=["AUDIO"]
            )
        )
        
        # Save audio response
        if hasattr(response, 'audio_data'):
            with open(output_path, "wb") as f:
                f.write(response.audio_data)
            return output_path
        else:
            return None
            
    except Exception as e:
        print(f"BGM generation failed: {e}")
        return None
